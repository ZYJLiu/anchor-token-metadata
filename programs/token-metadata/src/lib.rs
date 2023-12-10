use anchor_lang::{
    prelude::*,
    system_program::{create_account, transfer, CreateAccount, Transfer},
};
use anchor_spl::token_interface::Mint;
use solana_program::borsh0_10::get_instance_packed_len;
use spl_pod::optional_keys::OptionalNonZeroPubkey;
use spl_token_metadata_interface::{
    error::TokenMetadataError,
    instruction::TokenMetadataInstruction,
    state::{Field, TokenMetadata},
};
use spl_type_length_value::state::{
    realloc_and_pack_first_variable_len, TlvState, TlvStateBorrowed, TlvStateMut,
};

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
                msg!("Instruction: Anchor Initialize");
                let serialized_data = data
                    .try_to_vec()
                    .map_err(|_| ProgramError::InvalidInstructionData)?;

                __private::__global::initialize(program_id, accounts, &serialized_data)
            }
            TokenMetadataInstruction::UpdateField(data) => {
                msg!("Instruction: Anchor UpdateField");
                let serialized_data = data
                    .try_to_vec()
                    .map_err(|_| ProgramError::InvalidInstructionData)?;
                __private::__global::update_field(program_id, accounts, &serialized_data)
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

    pub fn update_field(ctx: Context<UpdateField>, data: UpdateFieldData) -> Result<()> {
        // Get current TokenMetadata.
        let mut token_metadata = {
            let buffer = ctx.accounts.metadata.try_borrow_data()?;
            let state = TlvStateBorrowed::unpack(&buffer)?;
            state.get_first_variable_len_value::<TokenMetadata>()?
        };

        // Check update authority.
        let update_authority = Option::<Pubkey>::from(token_metadata.update_authority)
            .ok_or_else(|| ProgramError::Custom(TokenMetadataError::ImmutableMetadata as u32))?;
        msg!("Update authority: {:?}", update_authority);
        if update_authority != *ctx.accounts.update_authority.key {
            return Err(
                ProgramError::Custom(TokenMetadataError::IncorrectUpdateAuthority as u32).into(),
            );
        }

        // Perform the update on the TokenMetadata.
        let field = data.field.to_field();
        token_metadata.update(field, data.value);
        msg!("TokenMetadata: {:?}", token_metadata);

        // Calculate the required size and lamports for the updated metadata.
        let new_size = TokenMetadata::tlv_size_of(&token_metadata)?;
        let required_lamports = Rent::get()?.minimum_balance(new_size as usize);

        // Get current state of the metadata account.
        let metadata_account_info = ctx.accounts.metadata.to_account_info();
        let current_lamports = metadata_account_info.lamports();

        // Transfer lamports if required.
        if required_lamports != current_lamports {
            let lamport_difference =
                (required_lamports as isize - current_lamports as isize).abs() as u64;
            if required_lamports > current_lamports {
                // Transfer additional lamports to metadata account.
                msg!(
                    "Transferring {} lamports to metadata account",
                    lamport_difference
                );
                transfer(
                    CpiContext::new(
                        ctx.accounts.system_program.to_account_info(),
                        Transfer {
                            from: ctx.accounts.payer.to_account_info(),
                            to: metadata_account_info.clone(),
                        },
                    ),
                    lamport_difference,
                )?;
            } else {
                // Transfer excess lamports back to payer.
                msg!("Transferring {} lamports back to payer", lamport_difference);
                // Modify lamports directly because metadata account is owned by this program (and not System Program)
                ctx.accounts.metadata.sub_lamports(lamport_difference)?;
                ctx.accounts.payer.add_lamports(lamport_difference)?;
            }
        }

        // Reallocate and update the metadata account data.
        realloc_and_pack_first_variable_len(
            &ctx.accounts.metadata.to_account_info(),
            &token_metadata,
        )?;
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

#[derive(Accounts)]
pub struct UpdateField<'info> {
    /// CHECK: check by address only, no anchor type to check against
    #[account(
        seeds = [b"metadata", mint.key().as_ref()],
        bump)
    ]
    pub metadata: UncheckedAccount<'info>,
    pub update_authority: Signer<'info>,
    pub mint: InterfaceAccount<'info, Mint>,
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(AnchorSerialize, AnchorDeserialize)]
pub struct UpdateFieldData {
    /// Field to update in the metadata
    pub field: AnchorField,
    /// Value to write for the field
    pub value: String,
}

// Need to do this so the enum shows up in the IDL
#[derive(AnchorSerialize, AnchorDeserialize)]
pub enum AnchorField {
    /// The name field, corresponding to `TokenMetadata.name`
    Name,
    /// The symbol field, corresponding to `TokenMetadata.symbol`
    Symbol,
    /// The uri field, corresponding to `TokenMetadata.uri`
    Uri,
    /// A user field, whose key is given by the associated string
    Key(String),
}

// Convert AnchorField to Field from spl_token_metadata_interface
impl AnchorField {
    fn to_field(&self) -> Field {
        match self {
            AnchorField::Name => Field::Name,
            AnchorField::Symbol => Field::Symbol,
            AnchorField::Uri => Field::Uri,
            AnchorField::Key(s) => Field::Key(s.clone()),
        }
    }
}
