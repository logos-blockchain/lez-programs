use std::{
    env,
    error::Error,
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use borsh::BorshSerialize;
use logos_scaffold::api::{
    testnode::{
        AccountValue, PinOverrides, ProofValue, ReadAt, RejectionPhase, TestNode, TestNodeClient,
        TestNodeConfig, TransactionBytes, TransactionOutcome, WaitOptions,
    },
    Project,
};
use nssa::{
    error::LeeError, privacy_preserving_transaction::PrivacyPreservingTransaction,
    program_deployment_transaction::ProgramDeploymentTransaction,
    public_transaction::PublicTransaction,
};
use nssa_core::{
    account::{Account, AccountId},
    Commitment, MembershipProof, Nullifier, Timestamp,
};
use rocksdb::{ColumnFamilyDescriptor, DBWithThreadMode, MultiThreaded, Options};

type DynError = Box<dyn Error + Send + Sync>;
type DynResult<T> = Result<T, DynError>;

const TEST_NODE_CIRCUITS_VERSION_ENV: &str = "LOGOS_SCAFFOLD_TEST_NODE_CIRCUITS_VERSION";
const RISC0_BUILD_DEBUG_ENV: &str = "RISC0_BUILD_DEBUG";
const DEFAULT_CIRCUITS_VERSION: &str = "0.4.2";
const CF_BLOCK_NAME: &str = "cf_block";
const CF_META_NAME: &str = "cf_meta";
const CF_NSSA_STATE_NAME: &str = "cf_nssa_state";
const DB_NSSA_STATE_KEY: &str = "nssa_state";
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const COMMIT_TIMEOUT: Duration = Duration::from_secs(20);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

pub struct TestState {
    inner: nssa::V03State,
    sequencer: Option<LocalSequencer>,
    dirty: bool,
}

impl TestState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: nssa::V03State::new(),
            sequencer: None,
            dirty: true,
        }
    }

    #[must_use]
    pub fn new_with_genesis_accounts(
        public_accounts: &[(AccountId, u128)],
        private_accounts: Vec<(Commitment, Nullifier)>,
        _genesis_timestamp: Timestamp,
    ) -> Self {
        Self {
            inner: nssa::V03State::new()
                .with_public_account_balances(public_accounts.iter().copied())
                .with_private_accounts(private_accounts),
            sequencer: None,
            dirty: true,
        }
    }

    pub fn transition_from_public_transaction(
        &mut self,
        tx: &PublicTransaction,
        block_id: u64,
        timestamp: Timestamp,
    ) -> Result<(), LeeError> {
        let rpc_tx = RpcTransaction::Public(Box::new(tx.clone()));
        match self.mirror_transaction(&rpc_tx) {
            MirrorOutcome::Committed(context) => {
                let mut next = self.inner.clone();
                next.transition_from_public_transaction(tx, context.block_id, context.timestamp)
                    .unwrap_or_else(|err| {
                        panic!(
                            "local replay rejected public transaction committed by sequencer at \
                             block {}: {err}",
                            context.block_id
                        )
                    });
                self.inner = next;
                self.assert_affected_accounts_match(&rpc_tx);
                Ok(())
            }
            MirrorOutcome::NotCommitted(rejection) => {
                let mut expected = self.inner.clone();
                let result = if let Some(context) = rejection.validation_context {
                    expected.transition_from_public_transaction(
                        tx,
                        context.block_id,
                        context.timestamp,
                    )
                } else {
                    expected.transition_from_public_transaction(tx, block_id, timestamp)
                };

                if result.is_ok() {
                    panic!("local replay accepted public transaction dropped by sequencer");
                }
                result
            }
        }
    }

    pub fn transition_from_privacy_preserving_transaction(
        &mut self,
        tx: &PrivacyPreservingTransaction,
        block_id: u64,
        timestamp: Timestamp,
    ) -> Result<(), LeeError> {
        let rpc_tx = RpcTransaction::PrivacyPreserving(Box::new(tx.clone()));
        match self.mirror_transaction(&rpc_tx) {
            MirrorOutcome::Committed(context) => {
                let mut next = self.inner.clone();
                next.transition_from_privacy_preserving_transaction(
                    tx,
                    context.block_id,
                    context.timestamp,
                )
                .unwrap_or_else(|err| {
                    panic!(
                        "local replay rejected privacy-preserving transaction committed by \
                         sequencer at block {}: {err}",
                        context.block_id
                    )
                });
                self.inner = next;
                self.assert_affected_accounts_match(&rpc_tx);
                Ok(())
            }
            MirrorOutcome::NotCommitted(rejection) => {
                let mut expected = self.inner.clone();
                let result = if let Some(context) = rejection.validation_context {
                    expected.transition_from_privacy_preserving_transaction(
                        tx,
                        context.block_id,
                        context.timestamp,
                    )
                } else {
                    expected.transition_from_privacy_preserving_transaction(tx, block_id, timestamp)
                };

                if result.is_ok() {
                    panic!(
                        "local replay accepted privacy-preserving transaction dropped by sequencer"
                    );
                }
                result
            }
        }
    }

    pub fn transition_from_program_deployment_transaction(
        &mut self,
        tx: &ProgramDeploymentTransaction,
    ) -> Result<(), LeeError> {
        let rpc_tx = RpcTransaction::ProgramDeployment(Box::new(tx.clone()));
        match self.mirror_transaction(&rpc_tx) {
            MirrorOutcome::Committed(context) => {
                let mut next = self.inner.clone();
                next.transition_from_program_deployment_transaction(tx)
                    .unwrap_or_else(|err| {
                        panic!(
                            "local replay rejected program deployment committed by sequencer at \
                             block {}: {err}",
                            context.block_id
                        )
                    });
                self.inner = next;
                self.assert_affected_accounts_match(&rpc_tx);
                Ok(())
            }
            MirrorOutcome::NotCommitted(_) => {
                let mut expected = self.inner.clone();
                let result = expected.transition_from_program_deployment_transaction(tx);
                if result.is_ok() {
                    panic!("local replay accepted program deployment dropped by sequencer");
                }
                result
            }
        }
    }

    pub fn force_insert_account(&mut self, account_id: AccountId, account: Account) {
        self.inner.force_insert_account(account_id, account);
        self.dirty = true;
    }

    #[must_use]
    pub fn get_account_by_id(&self, account_id: AccountId) -> Account {
        let account = self.inner.get_account_by_id(account_id);
        if !self.dirty {
            if let Some(sequencer) = &self.sequencer {
                let sequencer_account = sequencer
                    .get_account_by_id(account_id)
                    .unwrap_or_else(|err| panic!("local sequencer getAccount failed: {err}"));
                assert_eq!(
                    sequencer_account, account,
                    "local sequencer account state diverged for {account_id}"
                );
            }
        }
        account
    }

    #[must_use]
    pub fn get_proof_for_commitment(&self, commitment: &Commitment) -> Option<MembershipProof> {
        let proof = self.inner.get_proof_for_commitment(commitment);
        if !self.dirty {
            if let Some(sequencer) = &self.sequencer {
                let sequencer_proof = sequencer
                    .get_proof_for_commitment(commitment)
                    .unwrap_or_else(|err| {
                        panic!("local sequencer getProofForCommitment failed: {err}")
                    });
                assert_eq!(
                    sequencer_proof, proof,
                    "local sequencer commitment proof diverged"
                );
            }
        }
        proof
    }

    fn mirror_transaction(&mut self, tx: &RpcTransaction) -> MirrorOutcome {
        let tx_hash = hex_encode(&tx.hash());
        let outcome = self
            .ensure_sequencer()
            .submit_and_wait(tx, COMMIT_TIMEOUT)
            .unwrap_or_else(|err| panic!("local sequencer failed to submit {tx_hash}: {err}"));

        match outcome {
            TransactionOutcome::Committed { block, .. } => {
                MirrorOutcome::Committed(ObservedBlock {
                    block_id: block.block_id,
                    timestamp: block.timestamp,
                })
            }
            TransactionOutcome::Rejected {
                phase: RejectionPhase::Stateless,
                ..
            } => MirrorOutcome::NotCommitted(RejectionContext::precheck()),
            TransactionOutcome::Rejected {
                phase: RejectionPhase::Stateful,
                observed_after_block_id,
                ..
            } => {
                let validation_context = observed_after_block_id
                    .and_then(|block_id| self.sequencer.as_ref()?.block_context(block_id).ok());
                MirrorOutcome::NotCommitted(RejectionContext { validation_context })
            }
            TransactionOutcome::Timeout {
                last_observed_block_id,
                ..
            } => {
                panic!(
                    "local sequencer timed out waiting for {tx_hash}; last observed block \
                     {last_observed_block_id}"
                )
            }
            TransactionOutcome::TransportError { operation, message } => {
                panic!(
                    "local sequencer transport error during {operation} for {tx_hash}: {message}"
                )
            }
            TransactionOutcome::WireMismatch { .. } => {
                panic!("local sequencer echoed different transaction bytes for {tx_hash}")
            }
        }
    }

    fn ensure_sequencer(&mut self) -> &mut LocalSequencer {
        if self.sequencer.is_none() || self.dirty {
            self.sequencer = Some(
                LocalSequencer::spawn(&self.inner)
                    .unwrap_or_else(|err| panic!("failed to start local sequencer: {err}")),
            );
            self.dirty = false;
        }

        match &mut self.sequencer {
            Some(sequencer) => sequencer,
            None => unreachable!("local sequencer should be initialized"),
        }
    }

    fn assert_affected_accounts_match(&self, tx: &RpcTransaction) {
        let Some(sequencer) = &self.sequencer else {
            return;
        };

        let account_ids = tx.affected_public_account_ids();
        let sequencer_accounts = sequencer
            .get_accounts_by_id(&account_ids)
            .unwrap_or_else(|err| panic!("local sequencer batch getAccount failed: {err}"));
        for (account_id, sequencer_account) in account_ids.into_iter().zip(sequencer_accounts) {
            let account = self.inner.get_account_by_id(account_id);
            assert_eq!(
                sequencer_account, account,
                "local sequencer account state diverged for {account_id}"
            );
        }
    }
}

enum MirrorOutcome {
    Committed(ObservedBlock),
    NotCommitted(RejectionContext),
}

struct RejectionContext {
    validation_context: Option<ObservedBlock>,
}

impl RejectionContext {
    const fn precheck() -> Self {
        Self {
            validation_context: None,
        }
    }
}

#[derive(Clone, Copy)]
struct ObservedBlock {
    block_id: u64,
    timestamp: Timestamp,
}

fn ensure_risc0_dev_mode() -> DynResult<()> {
    if let Some(value) = env::var_os("RISC0_DEV_MODE") {
        let value = value.to_string_lossy();
        if matches!(value.trim(), "0" | "false") {
            return Err(io::Error::other(format!(
                "RISC0_DEV_MODE={value} disables dev mode, but the local sequencer harness requires it; unset RISC0_DEV_MODE or set it to 1"
            ))
            .into());
        }
    }
    Ok(())
}

fn ensure_release_guest_builds() -> DynResult<()> {
    if let Some(value) = env::var_os(RISC0_BUILD_DEBUG_ENV) {
        let value = value.to_string_lossy();
        if value.trim() == "1" {
            return Err(io::Error::other(format!(
                "{RISC0_BUILD_DEBUG_ENV}=1 enables debug-profile guest ELFs, but local sequencer tests require release-profile guest ELFs; unset {RISC0_BUILD_DEBUG_ENV} or set it to 0"
            ))
            .into());
        }
    }
    Ok(())
}

struct LocalSequencer {
    // Fields drop in declaration order; shut the node down before seed cleanup.
    _node: TestNode,
    client: TestNodeClient,
    _seed_dir: SeedDirGuard,
}

impl LocalSequencer {
    fn spawn(state: &nssa::V03State) -> DynResult<Self> {
        ensure_risc0_dev_mode()?;
        ensure_release_guest_builds()?;
        let seed_dir = SeedDirGuard::from_state(state)?;
        let config = TestNodeConfig {
            state: Some(seed_dir.path().to_path_buf()),
            timeout_sec: HEALTH_TIMEOUT.as_secs(),
            ..Default::default()
        };
        let node = TestNode::start(scaffold_project(), &config)
            .map_err(|err| io::Error::other(format!("scaffold test-node start failed: {err}")))?;
        Ok(Self {
            client: node.client(),
            _node: node,
            _seed_dir: seed_dir,
        })
    }

    fn submit_and_wait(
        &self,
        tx: &RpcTransaction,
        timeout: Duration,
    ) -> DynResult<TransactionOutcome> {
        let bytes = TransactionBytes::borsh(borsh::to_vec(tx)?);
        Ok(self.client.submit_and_wait(
            &bytes,
            &WaitOptions {
                timeout,
                rejection_blocks: 1,
                poll_interval: POLL_INTERVAL,
                ..Default::default()
            },
        ))
    }

    fn block_context(&self, block_id: u64) -> DynResult<ObservedBlock> {
        let block = self.client.block_info(block_id)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("sequencer block {block_id} was not found"),
            )
        })?;
        Ok(ObservedBlock {
            block_id: block.block_id,
            timestamp: block.timestamp,
        })
    }

    fn get_account_by_id(&self, account_id: AccountId) -> DynResult<Account> {
        let read = self
            .client
            .account(&account_id.to_string(), ReadAt::Latest)?;
        account_from_value(account_id, read.value)
    }

    fn get_accounts_by_id(&self, account_ids: &[AccountId]) -> DynResult<Vec<Account>> {
        let ids = account_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let read = self.client.accounts(&ids, ReadAt::Latest)?;
        read.accounts
            .into_iter()
            .zip(account_ids)
            .map(|(entry, account_id)| account_from_value(*account_id, entry.value))
            .collect()
    }

    fn get_proof_for_commitment(
        &self,
        commitment: &Commitment,
    ) -> DynResult<Option<MembershipProof>> {
        let commitment_hex = hex_encode(&commitment.to_byte_array());
        let read = self.client.proof(&commitment_hex, ReadAt::Latest)?;
        read.proof.map(proof_from_value).transpose()
    }
}

struct SeedDirGuard {
    path: PathBuf,
}

impl SeedDirGuard {
    fn from_state(state: &nssa::V03State) -> DynResult<Self> {
        let path = local_sequencer_seed_dir()?;
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        seed_sequencer_state(&path, state)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SeedDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(BorshSerialize)]
struct NssaStateCellRef<'state>(&'state nssa::V03State);

fn seed_sequencer_state(seed_dir: &Path, state: &nssa::V03State) -> DynResult<()> {
    // Scaffold snapshot seeding cannot represent full public account data at
    // this LEZ pin, while these fixtures use force-inserted public accounts.
    let mut cf_opts = Options::default();
    cf_opts.set_max_write_buffer_number(16);
    let cfb = ColumnFamilyDescriptor::new(CF_BLOCK_NAME, cf_opts.clone());
    let cfmeta = ColumnFamilyDescriptor::new(CF_META_NAME, cf_opts.clone());
    let cfstate = ColumnFamilyDescriptor::new(CF_NSSA_STATE_NAME, cf_opts);

    let mut db_opts = Options::default();
    db_opts.create_missing_column_families(true);
    db_opts.create_if_missing(true);
    let db = DBWithThreadMode::<MultiThreaded>::open_cf_descriptors(
        &db_opts,
        seed_dir.join("rocksdb"),
        vec![cfb, cfmeta, cfstate],
    )?;
    let state_column = db
        .cf_handle(CF_NSSA_STATE_NAME)
        .ok_or_else(|| io::Error::other("state column family not created"))?;
    db.put_cf(
        &state_column,
        borsh::to_vec(&DB_NSSA_STATE_KEY)?,
        borsh::to_vec(&NssaStateCellRef(state))?,
    )?;
    Ok(())
}

fn scaffold_project() -> &'static Project {
    static PROJECT: OnceLock<Project> = OnceLock::new();

    PROJECT.get_or_init(|| {
        let root = repo_root();
        let project = Project::open(&root).unwrap_or_else(|err| {
            panic!(
                "failed to open scaffold project at {}: {err}",
                root.display()
            )
        });
        let circuits_version = env::var(TEST_NODE_CIRCUITS_VERSION_ENV)
            .unwrap_or_else(|_| DEFAULT_CIRCUITS_VERSION.to_owned());
        logos_scaffold::api::testnode::prepare_test_node(
            &project,
            &PinOverrides {
                circuits_version: Some(circuits_version),
                ..Default::default()
            },
            None,
        )
        .unwrap_or_else(|err| panic!("failed to prepare scaffold test node: {err}"));
        project
    })
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("integration_tests crate lives under programs/integration_tests")
        .to_path_buf()
}

fn local_sequencer_seed_dir() -> DynResult<PathBuf> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(repo_root().join(format!(
        "target/local-sequencer/seeds/{}-{timestamp}",
        std::process::id()
    )))
}

fn account_from_value(account_id: AccountId, value: AccountValue) -> DynResult<Account> {
    match value {
        AccountValue::Missing => Ok(Account::default()),
        AccountValue::Present { encoded, .. } => {
            let bytes = BASE64_STANDARD.decode(encoded)?;
            Ok(borsh::from_slice(&bytes)?)
        }
        AccountValue::DecodeError { message, .. } => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("sequencer account {account_id} could not be decoded: {message}"),
        )
        .into()),
    }
}

fn proof_from_value(proof: ProofValue) -> DynResult<MembershipProof> {
    let leaf_index = usize::try_from(proof.leaf_index).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("proof leaf index {} does not fit usize", proof.leaf_index),
        )
    })?;
    let path = proof
        .path
        .iter()
        .map(|node| parse_hex_32(node))
        .collect::<DynResult<Vec<_>>>()?;
    Ok((leaf_index, path))
}

fn parse_hex_32(value: &str) -> DynResult<[u8; 32]> {
    let bytes = value.as_bytes();
    if bytes.len() != 64 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected 64 hex chars, got {}", bytes.len()),
        )
        .into());
    }

    let mut out = [0_u8; 32];
    for (index, chunk) in bytes.chunks_exact(2).enumerate() {
        let slot = out
            .get_mut(index)
            .ok_or_else(|| io::Error::other("hex output index out of bounds"))?;
        let pair = std::str::from_utf8(chunk)?;
        *slot = u8::from_str_radix(pair, 16)?;
    }
    Ok(out)
}

#[derive(BorshSerialize)]
enum RpcTransaction {
    Public(Box<PublicTransaction>),
    PrivacyPreserving(Box<PrivacyPreservingTransaction>),
    ProgramDeployment(Box<ProgramDeploymentTransaction>),
}

impl RpcTransaction {
    fn hash(&self) -> [u8; 32] {
        match self {
            Self::Public(tx) => tx.hash(),
            Self::PrivacyPreserving(tx) => tx.hash(),
            Self::ProgramDeployment(tx) => tx.hash(),
        }
    }

    fn affected_public_account_ids(&self) -> Vec<AccountId> {
        match self {
            Self::Public(tx) => tx.affected_public_account_ids(),
            Self::PrivacyPreserving(tx) => tx.affected_public_account_ids(),
            Self::ProgramDeployment(tx) => tx.affected_public_account_ids(),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
