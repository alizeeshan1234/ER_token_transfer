use anchor_lang::prelude::*;
use ephemeral_rollups_sdk::anchor::{commit, delegate, ephemeral};
use ephemeral_rollups_sdk::cpi::DelegateConfig;
use ephemeral_rollups_sdk::ephem::{commit_accounts, commit_and_undelegate_accounts};
use anchor_spl::{associated_token::AssociatedToken, token::{Mint, Token, TokenAccount, TransferChecked, transfer_checked}, *};

declare_id!("sXhfCv6D62CWq86U468hGjDou6kLzehQgYUPEWqZd3U");

#[ephemeral]
#[program]
pub mod token_transfer_er {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }

    pub fn create_token_escrow(ctx: Context<CreateTokenEscrow>) -> Result<()> {

        let token_escrow_account_info = &mut ctx.accounts.token_escrow;

        token_escrow_account_info.authority = ctx.accounts.authority.key();
        token_escrow_account_info.mint = ctx.accounts.mint.key();
        token_escrow_account_info.escrow_token_account = ctx.accounts.escrow_token_account.key();
        token_escrow_account_info.balance = 0;
        token_escrow_account_info.is_delegated = false;
        token_escrow_account_info.bump = ctx.bumps.token_escrow;

        msg!("Token Escrow Account Info: {:?}", token_escrow_account_info);

        Ok(())
    }

    pub fn process_token_escrow_deposit(ctx: Context<EscrowDeposit>, amount: u64) -> Result<()> {

        let token_escrow_account = &mut ctx.accounts.token_escrow;

        require!(token_escrow_account.authority == ctx.accounts.authority.key(), ErrorCode::InvalidAuthority);
        require!(token_escrow_account.is_delegated == false, ErrorCode::AccountAlreadyDelegated);
        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(ctx.accounts.user_token_account.amount >= amount, ErrorCode::InsufficientBalance);

        let cpi_accounts = TransferChecked {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.escrow_token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            authority: ctx.accounts.authority.to_account_info()
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_context = CpiContext::new(cpi_program, cpi_accounts);

        transfer_checked(cpi_context, amount, ctx.accounts.mint.decimals)?;

        token_escrow_account.balance = token_escrow_account.balance.checked_add(amount).ok_or(ErrorCode::MathOverflow)?;

        msg!("Deposited {} tokens to escrow", amount);
        msg!("New escrow balance: {}", token_escrow_account.balance);

        Ok(())

    }

    pub fn delegate_escrow(ctx: Context<DelegateAccount>, commet_frequency: u32, validator_key: Pubkey) -> Result<()> {

        let delegate_config = DelegateConfig {
            commit_frequency_ms: commet_frequency,
            validator: Some(validator_key), 
        };

        let mint_ref = ctx.accounts.mint.key();
        let authority_ref = ctx.accounts.payer.key();
        let seeds = &[
            b"token_escrow",
            mint_ref.as_ref(),
            authority_ref.as_ref()
        ];

        ctx.accounts.delegate_token_escrow(&ctx.accounts.payer, seeds, delegate_config)?;

        msg!("Token Escrow Account Delegated to ER");

        Ok(())
    }

    pub fn token_escrow_transfer_er(ctx: Context<TokenEscrowTransferER>, amount: u64) -> Result<()> {

        require!(amount > 0, ErrorCode::InvalidAmount);
        require!(ctx.accounts.sender_escrow_account.balance >= amount, ErrorCode::InsufficientBalance);

        ctx.accounts.sender_escrow_account.balance = ctx.accounts.sender_escrow_account.balance
            .checked_sub(amount)
            .ok_or(ErrorCode::MathOverflow)?;

        ctx.accounts.recever_escrow_account.balance = ctx.accounts.recever_escrow_account.balance
            .checked_add(amount)
            .ok_or(ErrorCode::MathOverflow)?;

        msg!(
            "Transferred {} tokens from {} to {}",
            amount,
            ctx.accounts.sender.key(),
            ctx.accounts.receiver.key()
        );
        msg!("From balance: {}", ctx.accounts.sender_escrow_account.balance);
        msg!("To balance: {}", ctx.accounts.recever_escrow_account.balance);
        Ok(())
    }

    pub fn process_commit_and_undelegate(ctx: Context<UndelegateAccount>) -> Result<()> {

        commit_and_undelegate_accounts(
            &ctx.accounts.payer,
            vec![
                &ctx.accounts.sender_token_escrow.to_account_info(),
                &ctx.accounts.receiver_token_escrow.to_account_info(),
            ], 
            &ctx.accounts.magic_context, 
            &ctx.accounts.magic_program
        )?;

        Ok(())
    }

    pub fn process_withdraw_from_escrow(ctx: Context<WithdrawFromEscrow>, amount: u64) -> Result<()> {

        let sender_escrow = &mut ctx.accounts.sender_token_escrow;
        let receiver_escrow = &mut ctx.accounts.receiver_token_escrow;

        require!(amount > 0, ErrorCode::InvalidAmount);
        // require!(sender_escrow.balance >= amount, ErrorCode::InsufficientBalance);
        require!(sender_escrow.is_delegated == false, ErrorCode::AccountAlreadyDelegated);
        require!(receiver_escrow.is_delegated == false, ErrorCode::AccountAlreadyDelegated);
        require!(ctx.accounts.sender_escrow_token_account.amount >= amount, ErrorCode::InsufficientBalance);

        let mint_ref = ctx.accounts.mint.key();
        let sender_ref = ctx.accounts.sender.key();
        let bump = sender_escrow.bump;
    
        let signer_seeds: &[&[&[u8]]] = &[&[
            b"token_escrow",
            mint_ref.as_ref(),
            sender_ref.as_ref(),
            &[bump],
        ]];

        let cpi_accounts = TransferChecked {
            from: ctx.accounts.sender_escrow_token_account.to_account_info(),
            to: ctx.accounts.receiver_escrow_token_account.to_account_info(),
            mint: ctx.accounts.mint.to_account_info(),
            authority: sender_escrow.to_account_info(), // PDA is the authority
        };

        let cpi_program = ctx.accounts.token_program.to_account_info();
        let cpi_context = CpiContext::new_with_signer(cpi_program, cpi_accounts, signer_seeds);

        transfer_checked(cpi_context, amount, ctx.accounts.mint.decimals)?;

        sender_escrow.balance = ctx.accounts.sender_escrow_token_account.amount.checked_sub(amount).unwrap();
        receiver_escrow.balance = ctx.accounts.receiver_escrow_token_account.amount.checked_add(amount).unwrap();

        msg!(
            "Transferred {} tokens on-chain from {} to {}",
            amount,
            ctx.accounts.sender.key(),
            ctx.accounts.receiver.key()
        );

        msg!("Sender escrow balance after: {}", sender_escrow.balance);
        msg!("Receiver escrow balance after: {}", receiver_escrow.balance);
        Ok(())
    }

}

#[derive(Accounts)]
pub struct Initialize {}

#[derive(Accounts)]
pub struct CreateTokenEscrow<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        init,
        payer = authority,
        space = 8 + TokenEscrow::INIT_SPACE,
        seeds = [b"token_escrow", mint.key().as_ref(), authority.key().as_ref()],
        bump
    )]
    pub token_escrow: Account<'info, TokenEscrow>,

    //Account where the user will deposit tokens
    #[account(
        init_if_needed,
        payer = authority,
        associated_token::mint = mint,
        associated_token::authority = token_escrow
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,

    pub system_program: Program<'info, System>,

    pub token_program: Program<'info, Token>,

    pub associated_token_program: Program<'info, AssociatedToken>
}

#[derive(Accounts)]
pub struct EscrowDeposit<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), authority.key().as_ref()],
        bump = token_escrow.bump,
    )]
    pub token_escrow: Account<'info, TokenEscrow>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = token_escrow
    )]
    pub escrow_token_account: Account<'info, TokenAccount>,

    //User token account from where tokens will be deposited
    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = authority,
    )]
    pub user_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[delegate]
#[derive(Accounts)]
pub struct DelegateAccount<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        del,
        seeds = [b"token_escrow", mint.key().as_ref(), payer.key().as_ref()],
        bump = token_escrow.bump,
    )]
    pub token_escrow: Account<'info, TokenEscrow>,
}


#[derive(Accounts)]
pub struct TokenEscrowTransferER<'info> {
    #[account(mut)]
    pub sender: Signer<'info>,

    pub receiver: AccountInfo<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), sender.key().as_ref()],
        bump,
    )]
    pub sender_escrow_account: Account<'info, TokenEscrow>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), receiver.key().as_ref()],
        bump,
    )]
    pub recever_escrow_account: Account<'info, TokenEscrow>,
}

#[commit]
#[derive(Accounts)]
pub struct UndelegateAccount<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub sender: AccountInfo<'info>,

    pub receiver: AccountInfo<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), sender.key().as_ref()],
        bump
    )]
    pub sender_token_escrow: Account<'info, TokenEscrow>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), receiver.key().as_ref()],
        bump
    )]
    pub receiver_token_escrow: Account<'info, TokenEscrow>,
}

#[derive(Accounts)]
pub struct WithdrawFromEscrow<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    pub sender: AccountInfo<'info>,

    pub receiver: AccountInfo<'info>,

    pub mint: Account<'info, Mint>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), sender.key().as_ref()],
        bump = sender_token_escrow.bump,
    )]
    pub sender_token_escrow: Account<'info, TokenEscrow>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = sender_token_escrow
    )]
    pub sender_escrow_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"token_escrow", mint.key().as_ref(), receiver.key().as_ref()],
        bump = receiver_token_escrow.bump,
    )]
    pub receiver_token_escrow: Account<'info, TokenEscrow>,

    #[account(
        mut,
        associated_token::mint = mint,
        associated_token::authority = receiver_token_escrow
    )]
    pub receiver_escrow_token_account: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[account]
#[derive(Debug, InitSpace)]
pub struct TokenEscrow {
    pub authority: Pubkey,
    pub mint: Pubkey,
    pub escrow_token_account: Pubkey,
    pub balance: u64,
    pub is_delegated: bool,
    pub bump: u8,
}

#[error_code]
pub enum ErrorCode {
    #[msg("Custom error message")]
    CustomError,
    InsufficientFunds,
    InvalidAuthority,
    AccountAlreadyDelegated,
    InvalidAmount,
    InsufficientBalance,
    MathOverflow
}