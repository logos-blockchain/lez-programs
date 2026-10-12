#![cfg_attr(not(test), no_main)]

use ata_core::AtaContents;
use nssa_core::account::{AccountId, ProgramShardSelector, ShardData};
use spel_framework::{context::ProgramContext, prelude::*};
use token_core::{TokenDescriptor, TokenKind};

#[cfg(not(test))]
risc0_zkvm::guest::entry!(main);

// This program owns no shard. Every effect it emits names a *Token Program*
// shard, so each one is a read: the apply half runs here, with those bytes, and
// must return `None`. That is what makes a guard possible across a program
// boundary — the shard's owner decides who may write it, but anyone may look.
#[lez_program(instruction = "ata_core::Instruction")]
mod ata {
    #[expect(
        unused_imports,
        reason = "SPEL instruction macro requires importing parent-scope handler types"
    )]
    use super::*;

    /// Create the Associated Token Account for (token program, owner, definition).
    ///
    /// Idempotent, which is the awkward part: whether to chain an initialize
    /// depends on what the ATA already holds, and the plan phase cannot see that.
    /// So the caller declares it as `contents` and the guard below pins the claim
    /// before anything branches on it.
    #[plan]
    pub fn create(
        ctx: ProgramContext,
        owner: AccountMeta,
        token_definition: AccountMeta,
        #[account(mut)] ata_account: AccountMeta,
        token_program_id: AccountId,
        kind: TokenKind,
        contents: AtaContents,
    ) -> SpelResult {
        let (seed, needs_initialize) = ata_program::create::plan_create(
            &owner,
            &token_definition,
            &ata_account,
            ctx.self_account_id,
            token_program_id,
            contents,
        );
        let descriptor = TokenDescriptor {
            definition_id: token_definition.account_id,
            kind,
        };

        let mut output = SpelOutput::empty().guard(
            &ata_account,
            token_program_id,
            &Effect::CheckContents(ata_core::CheckContents {
                descriptor,
                contents,
            }),
        );

        if needs_initialize {
            output = output.call(
                ChainedCall::new(
                    token_program_id,
                    vec![
                        ProgramShardSelector::new(token_definition.account_id, token_program_id),
                        ProgramShardSelector::new(ata_account.account_id, token_program_id),
                    ],
                    &token_core::Instruction::InitializeAccount { kind },
                )
                .with_pda_seeds(vec![seed]),
            );
        } else {
            // The only branch with no child call, so the only one where nothing
            // downstream pins `kind` to the definition it was classified against.
            output = output.guard(
                &token_definition,
                token_program_id,
                &Effect::CheckDefinitionKind(ata_core::CheckDefinitionKind { kind }),
            );
        }

        Ok(output)
    }

    /// Transfer tokens FROM owner's ATA to a recipient token holding account.
    #[plan]
    pub fn transfer(
        ctx: ProgramContext,
        owner: AccountMeta,
        #[account(mut)] sender_ata: AccountMeta,
        #[account(mut)] recipient: AccountMeta,
        token_program_id: AccountId,
        descriptor: TokenDescriptor,
        amount: u128,
    ) -> SpelResult {
        let seed = ata_program::transfer::plan_transfer(
            &owner,
            &sender_ata,
            ctx.self_account_id,
            token_program_id,
            &descriptor,
        );

        Ok(SpelOutput::empty()
            .guard(
                &recipient,
                token_program_id,
                &Effect::CheckRecipient(ata_core::CheckRecipient { descriptor }),
            )
            .call(
                ChainedCall::new(
                    token_program_id,
                    vec![
                        ProgramShardSelector::new(sender_ata.account_id, token_program_id),
                        ProgramShardSelector::new(recipient.account_id, token_program_id),
                    ],
                    &token_core::Instruction::Transfer {
                        amount_to_transfer: amount,
                        descriptor,
                    },
                )
                .with_pda_seeds(vec![seed]),
            ))
    }

    /// Burn tokens FROM owner's ATA.
    #[plan]
    pub fn burn(
        ctx: ProgramContext,
        owner: AccountMeta,
        #[account(mut)] holder_ata: AccountMeta,
        #[account(mut)] token_definition: AccountMeta,
        token_program_id: AccountId,
        descriptor: TokenDescriptor,
        amount: u128,
    ) -> SpelResult {
        let seed = ata_program::burn::plan_burn(
            &owner,
            &holder_ata,
            &token_definition,
            ctx.self_account_id,
            token_program_id,
            &descriptor,
        );

        Ok(SpelOutput::empty().call(
            ChainedCall::new(
                token_program_id,
                vec![
                    ProgramShardSelector::new(token_definition.account_id, token_program_id),
                    ProgramShardSelector::new(holder_ata.account_id, token_program_id),
                ],
                &token_core::Instruction::Burn {
                    amount_to_burn: amount,
                    kind: descriptor.kind,
                },
            )
            .with_pda_seeds(vec![seed]),
        ))
    }

    // ── guards ──────────────────────────────────────────────────────────────
    //
    // Each returns `None`: writing a shard this program does not own is rejected
    // by the runtime, and a guard that accidentally wrote would be a silent hole.

    #[apply]
    pub fn check_contents(
        effect: ata_core::CheckContents,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        ata_program::create::check_contents(pre_data, &effect.descriptor, effect.contents);
        None
    }

    #[apply]
    pub fn check_definition_kind(
        effect: ata_core::CheckDefinitionKind,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        ata_program::create::check_definition_kind(pre_data, effect.kind);
        None
    }

    #[apply]
    pub fn check_recipient(
        effect: ata_core::CheckRecipient,
        pre_data: &ShardData,
    ) -> Option<ShardData> {
        ata_program::transfer::check_recipient(pre_data, &effect.descriptor);
        None
    }
}
