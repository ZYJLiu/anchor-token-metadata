use anchor_lang::{
    prelude::*,
    system_program::{create_account, CreateAccount},
};
use anchor_spl::token_interface::Mint;
use solana_program::borsh0_10::get_instance_packed_len;
use spl_pod::optional_keys::OptionalNonZeroPubkey;
use spl_token_metadata_interface::{instruction::TokenMetadataInstruction, state::TokenMetadata};
use spl_type_length_value::state::TlvStateMut;

declare_id!("9G9qb4bwYTywRLwXevYBMZ2AErdxAYUTnkaNf2t3RsgE");

#[program]
pub mod token_metadata {

    use super::*;

    pub fn fallback<'info>(
        program_id: &Pubkey,
        accounts: &'info [AccountInfo<'info>],
        data: &[u8],
    ) -> Result<()> {
        let instruction = TokenMetadataInstruction::unpack(data)?;

        match instruction {
            TokenMetadataInstruction::Initialize(data) => {
                msg!("Instruction: Initialize");
                let serialized_data = data
                    .try_to_vec()
                    .map_err(|_| ProgramError::InvalidInstructionData)?;

                __private::__global::initialize(program_id, accounts, &serialized_data)
            }
            _ => return Err(ProgramError::InvalidInstructionData.into()),
        }
    }

    pub fn initialize(ctx: Context<Initialize>, data: InitializeData) -> Result<()> {
        // Check if update authority is provided
        let update_authority_key = ctx
            .accounts
            .update_authority
            .as_ref()
            .map(|account| *account.key);
        let update_authority = OptionalNonZeroPubkey::try_from(update_authority_key)
            .map_err(|_| ProgramError::InvalidArgument)?;

        // Construct token metadata
        let token_metadata = TokenMetadata {
            name: data.name,
            symbol: data.symbol,
            uri: data.uri,
            update_authority,
            mint: ctx.accounts.mint.key(),
            ..Default::default()
        };
        msg!("TokenMetadata: {:?}", token_metadata);

        // Calculate size and lamports for the metadata account
        let size = TokenMetadata::tlv_size_of(&token_metadata)?;
        let lamports = Rent::get()?.minimum_balance(size as usize);

        // Create metadata account
        let mint = ctx.accounts.mint.key();
        let signer_seeds: &[&[&[u8]]] = &[&[b"metadata", mint.as_ref(), &[ctx.bumps.metadata]]];
        create_account(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                CreateAccount {
                    from: ctx.accounts.payer.to_account_info(),
                    to: ctx.accounts.metadata.to_account_info(),
                },
            )
            .with_signer(signer_seeds),
            lamports,
            size as u64,
            &id(),
        )?;

        // Initialize metadata account data
        let instance_size = get_instance_packed_len(&token_metadata)?;
        let mut buffer = ctx.accounts.metadata.try_borrow_mut_data()?;
        let mut state = TlvStateMut::unpack(&mut buffer)?;
        state.alloc::<TokenMetadata>(instance_size, false)?;
        state.pack_first_variable_len_value(&token_metadata)?;
        Ok(())
    }
}

// Order of the accounts in the struct matters
#[derive(Accounts)]
pub struct Initialize<'info> {
    /// CHECK: Create this account in instruction
    #[account(
        seeds = [b"metadata", mint.key().as_ref()], 
        bump)
    ]
    pub metadata: UncheckedAccount<'info>,
    /// CHECK: Optional update authority, unchecked because it can either be SystemAccount or a PDA owned by another program
    pub update_authority: Option<UncheckedAccount<'info>>,
    pub mint: InterfaceAccount<'info, Mint>,
    pub mint_authority: Signer<'info>,
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct InitializeData {
    pub name: String,
    pub symbol: String,
    pub uri: String,
}
