use integration_tests::{account, deploy_at, key, submit};
use lee::{error::LeeError, PrivateKey, V03State};
use lee_core::{
    account::{AccountId, Actor},
    program::TransactionEvent,
};
use nft_core::{MetadataStandard, NftMetadata, NftState};
use token_core::TokenState;

pub const ATA: AccountId = AccountId::new([0xa1; 32]);
pub const TOKEN: AccountId = AccountId::new([0xa2; 32]);
pub const NFT: AccountId = AccountId::new([0xa3; 32]);

pub struct Chain {
    pub state: V03State,
}

impl Chain {
    pub fn new() -> Self {
        Self {
            state: deploy_at(&[
                (ATA, ata_methods::ATA_ELF),
                (TOKEN, token_methods::TOKEN_ELF),
                (NFT, nft_methods::NFT_ELF),
            ]),
        }
    }

    pub fn call(
        &mut self,
        root: Actor,
        declared: &[Actor],
        signers: &[&PrivateKey],
        message: &impl borsh::BorshSerialize,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        submit(&mut self.state, root, declared, signers, message)
    }

    pub fn define(&mut self, nft: bool, supply: u128) -> AccountId {
        let (def, holder) = (key(10), key(11));
        let program = if nft { NFT } else { TOKEN };
        let actors = [
            Actor::new(account(&def), program),
            Actor::new(account(&holder), program),
        ];
        if nft {
            self.call(
                actors[0],
                &actors,
                &[&def, &holder],
                &nft_core::Message::NewDefinition {
                    name: "Art".into(),
                    printable_supply: supply,
                    master: account(&holder),
                    metadata: NftMetadata {
                        standard: MetadataStandard::Simple,
                        uri: "ipfs://art".into(),
                        creators: "Artist".into(),
                    },
                },
            )
            .expect("NFT definition initializes");
        } else {
            self.call(
                actors[0],
                &actors,
                &[&def, &holder],
                &token_core::Message::NewDefinition {
                    name: "Coin".into(),
                    total_supply: supply,
                    mint_authority: None,
                    holding: account(&holder),
                },
            )
            .expect("token definition initializes");
        }
        account(&def)
    }

    pub fn create(
        &mut self,
        program: AccountId,
        owner: AccountId,
        definition: AccountId,
    ) -> AccountId {
        let holding = ata(program, owner, definition);
        let root = Actor::new(account(&key(11)), ATA); // Already admitted, but not authorized.
        let declared = [
            root,
            Actor::new(holding, program),
            Actor::new(definition, program),
        ];
        let message = if program == NFT {
            ata_core::Message::CreateNft {
                nft_program_id: program,
                owner,
                definition,
            }
        } else {
            ata_core::Message::Create {
                token_program_id: program,
                owner,
                definition,
            }
        };
        self.call(root, &declared, &[], &message)
            .expect("permissionless ATA creation succeeds");
        holding
    }

    pub fn initialize(&mut self, program: AccountId, holder: &PrivateKey, definition: AccountId) {
        let root = Actor::new(account(holder), program);
        if program == NFT {
            self.call(
                root,
                &[root, Actor::new(definition, program)],
                &[holder],
                &nft_core::Message::InitializeHolding { definition },
            )
            .expect("NFT holding initializes");
        } else {
            self.call(
                root,
                &[root, Actor::new(definition, program)],
                &[holder],
                &token_core::Message::InitializeHolding { definition },
            )
            .expect("token holding initializes");
        }
    }

    pub fn fund(&mut self, program: AccountId, to: AccountId, amount: u128) {
        let holder = key(11);
        let root = Actor::new(account(&holder), program);
        if program == NFT {
            self.call(
                root,
                &[root, Actor::new(to, program)],
                &[&holder],
                &nft_core::Message::Transfer { to },
            )
            .expect("NFT master funds holding");
        } else {
            self.call(
                root,
                &[root, Actor::new(to, program)],
                &[&holder],
                &token_core::Message::Transfer { to, amount },
            )
            .expect("token transfer funds holding");
        }
    }

    pub fn token(&self, id: AccountId) -> TokenState {
        TokenState::decode(self.state.get_account_by_id(id).data.actor_state(TOKEN))
            .expect("initialized token state")
    }

    pub fn balance(&self, id: AccountId) -> u128 {
        let TokenState::Holding(holding) = self.token(id) else {
            panic!("holding")
        };
        holding.balance
    }

    pub fn nft(&self, id: AccountId) -> NftState {
        NftState::decode(self.state.get_account_by_id(id).data.actor_state(NFT))
            .expect("initialized NFT state")
    }

    pub fn transfer(
        &mut self,
        program: AccountId,
        owner: &PrivateKey,
        definition: AccountId,
        to: AccountId,
        amount: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let root = Actor::new(account(owner), ATA);
        let declared = [
            root,
            Actor::new(ata(program, account(owner), definition), program),
            Actor::new(to, ATA),
            Actor::new(to, program),
        ];
        let message = transfer_message(program, definition, to, amount);
        self.call(root, &declared, &[owner], &message)
    }

    pub fn burn(
        &mut self,
        program: AccountId,
        owner: &PrivateKey,
        definition: AccountId,
        amount: u128,
    ) -> Result<Vec<TransactionEvent>, LeeError> {
        let root = Actor::new(account(owner), ATA);
        let declared = [
            root,
            Actor::new(ata(program, account(owner), definition), program),
            Actor::new(definition, program),
        ];
        let message = burn_message(program, definition, amount);
        self.call(root, &declared, &[owner], &message)
    }

    pub fn idle(&self, id: AccountId) {
        assert!(self
            .state
            .get_account_by_id(id)
            .data
            .actor_state(ATA)
            .is_empty());
    }
}

pub fn ata(program: AccountId, owner: AccountId, definition: AccountId) -> AccountId {
    ata_core::get_associated_token_account_id(
        &ATA,
        &ata_core::compute_ata_seed(program, owner, definition),
    )
}

pub fn transfer_message(
    program: AccountId,
    definition: AccountId,
    to: AccountId,
    amount: u128,
) -> ata_core::Message {
    if program == NFT {
        ata_core::Message::TransferNft {
            nft_program_id: program,
            definition,
            to,
        }
    } else {
        ata_core::Message::Transfer {
            token_program_id: program,
            definition,
            to,
            amount,
        }
    }
}

pub fn burn_message(program: AccountId, definition: AccountId, amount: u128) -> ata_core::Message {
    if program == NFT {
        ata_core::Message::BurnNft {
            nft_program_id: program,
            definition,
            amount,
        }
    } else {
        ata_core::Message::Burn {
            token_program_id: program,
            definition,
            amount,
        }
    }
}
