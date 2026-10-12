use serde::Deserialize;
use serde_json::Value;

use crate::account::AccountRead;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgramIdRequest {
    pub elf: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodeDefinitionRequest {
    pub token_program_id: String,
    pub definition: AccountRead,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodeHoldingRequest {
    pub token_program_id: String,
    pub holding: AccountRead,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodeMetadataRequest {
    pub token_program_id: String,
    pub metadata: AccountRead,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DecodeAccountRequest {
    pub token_program_id: String,
    pub account: AccountRead,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateFungiblePlanRequest {
    pub token_program_id: String,
    pub definition_target_id: String,
    pub holding_target_id: String,
    pub name: String,
    pub total_supply_raw: Value,
    pub mint_authority: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateFungibleWithMetadataPlanRequest {
    pub token_program_id: String,
    pub definition_target_id: String,
    pub holding_target_id: String,
    pub metadata_target_id: String,
    pub name: String,
    pub total_supply_raw: Value,
    pub mint_authority: String,
    pub metadata_standard: String,
    pub uri: String,
    pub creators: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreateNonFungiblePlanRequest {
    pub token_program_id: String,
    pub definition_target_id: String,
    pub master_holding_target_id: String,
    pub metadata_target_id: String,
    pub name: String,
    pub printable_supply_raw: Value,
    pub metadata_standard: String,
    pub uri: String,
    pub creators: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InitializeHoldingPlanRequest {
    pub token_program_id: String,
    pub definition_id: String,
    pub holding_target_id: String,
    /// Which of the three holding shapes to create: `fungible`, `nftMaster` or
    /// `nftPrintedCopy`. Declared because the holding's own apply cannot see the
    /// definition it is being paired with, so the kind travels in the
    /// instruction and each side asserts it against its own shard.
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransferPlanRequest {
    pub token_program_id: String,
    pub sender_holding_id: String,
    pub recipient_holding_id: String,
    /// The definition being moved. Declared rather than read: neither side's
    /// apply sees the other's shard, so the descriptor is what makes "the same
    /// token left one holding and arrived at the other" a checked claim.
    pub definition_id: String,
    pub kind: String,
    pub amount_raw: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BurnPlanRequest {
    pub token_program_id: String,
    pub definition_id: String,
    pub holding_id: String,
    /// Which supply the definition decrements. It crosses between the two
    /// shards, so each side asserts it against its own contents.
    pub kind: String,
    pub amount_raw: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MintPlanRequest {
    pub token_program_id: String,
    pub definition_id: String,
    pub holding_id: String,
    pub amount_raw: Value,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MintWithAuthorityPlanRequest {
    pub token_program_id: String,
    pub definition_id: String,
    pub holding_id: String,
    pub authority_id: String,
    pub amount_raw: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetAuthorityPlanRequest {
    pub token_program_id: String,
    pub definition_id: String,
    pub new_authority: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetAuthorityWithAuthorityPlanRequest {
    pub token_program_id: String,
    pub definition_id: String,
    pub authority_id: String,
    pub new_authority: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrintNftPlanRequest {
    pub token_program_id: String,
    pub master_holding_id: String,
    pub printed_holding_target_id: String,
    /// The NFT definition both holdings belong to. Declared for the same reason
    /// as a transfer's descriptor: the printed copy's apply cannot see the
    /// master's shard.
    pub definition_id: String,
}
