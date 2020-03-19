use super::pots;
use super::{Entry, EntryOwned};
use crate::account::AccountAlg;
use crate::accounting::account::{
    AccountState, DelegationRatio, DelegationType, LastRewards, SpendingCounter,
};
use crate::block::ConsensusVersion;
use crate::certificate::{PoolId, PoolRegistration};
use crate::config::{ConfigParam, ConfigParamVariant, Tag};
use crate::date::BlockDate;
use crate::fee::{LinearFee, PerCertificateFee};
use crate::fragment::ConfigParams;
use crate::header::ChainLength;
use crate::key::serialize_public_key;
use crate::leadership::bft::LeaderId;
use crate::ledger::iter;
use crate::ledger::pots::EntryType;
use crate::ledger::{Globals, Ledger, LedgerStaticParameters};
use crate::multisig::{DeclElement, Declaration};
use crate::stake::{PoolLastRewards, PoolState};
use crate::update::{UpdateProposal, UpdateProposalId, UpdateProposalState, UpdateVoterId};
use crate::value::Value;
use crate::{config, key, multisig, utxo};
use cardano_legacy_address::Addr;
use chain_addr::{Address, Discrimination};
use chain_core::mempack::{ReadBuf, Readable};
use chain_crypto::digest::{DigestAlg, DigestOf};
use chain_crypto::AsymmetricPublicKey;
use chain_ser::deser::{Deserialize, Serialize};
use chain_ser::packer::Codec;
use chain_time::TimeEra;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::io::{Cursor, Error, Write};
use std::iter::FromIterator;
use std::sync::Arc;

fn pack_addr<W: std::io::Write>(addr: &Addr, codec: &mut Codec<W>) -> Result<(), std::io::Error> {
    codec.put_u64(addr.as_ref().len() as u64)?;
    for e in &addr.as_ref() {
        codec.put_u8(*e)?;
    }
    Ok(())
}

fn unpack_addr<R: std::io::BufRead>(codec: &mut Codec<R>) -> Result<Addr, std::io::Error> {
    let size = codec.get_u64()?;
    let mut v = Vec::with_capacity(size as usize);
    for _ in 0..size {
        v.push(codec.get_u8()?);
    }
    Ok(Addr(v))
}

fn pack_discrimination<W: std::io::Write>(
    discrimination: &Discrimination,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    match discrimination {
        Discrimination::Production => {
            codec.put_u8(0)?;
        }
        Discrimination::Test => {
            codec.put_u8(1)?;
        }
    };
    Ok(())
}

fn unpack_discrimination<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<Discrimination, std::io::Error> {
    match codec.get_u8()? {
        0 => Ok(Discrimination::Production),
        1 => Ok(Discrimination::Test),
        code => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Not recognize code {}", code),
        )),
    }
}

fn pack_digestof<H: DigestAlg, T, W: std::io::Write>(
    digestof: &DigestOf<H, T>,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    let inner_data = digestof.as_ref();
    codec.put_u64(inner_data.len() as u64)?;
    codec.put_bytes(inner_data)?;
    Ok(())
}

fn unpack_digestof<H: DigestAlg, T, R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<DigestOf<H, T>, std::io::Error> {
    let size = codec.get_u64()?;
    let bytes = codec.get_bytes(size as usize)?;
    match DigestOf::try_from(&bytes[..]) {
        Ok(data) => Ok(data),
        Err(e) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{}", e),
        )),
    }
}

fn pack_account_identifier<W: std::io::Write>(
    identifier: &crate::account::Identifier,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    serialize_public_key(&identifier.as_ref(), codec)
}

fn unpack_account_identifier<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<crate::account::Identifier, std::io::Error> {
    let bytes = codec.get_bytes(<AccountAlg as AsymmetricPublicKey>::PUBLIC_KEY_SIZE as usize)?;
    let mut bytes_buff = ReadBuf::from(&bytes);
    match crate::account::Identifier::read(&mut bytes_buff) {
        Ok(identifier) => Ok(identifier),
        Err(e) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Error reading Identifier: {}", e),
        )),
    }
}

fn pack_account_state<W: std::io::Write>(
    account_state: &AccountState<()>,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u32(account_state.counter.0)?;
    pack_delegation_type(&account_state.delegation, codec)?;
    codec.put_u64(account_state.value.0)?;
    pack_last_rewards(&account_state.last_rewards, codec)?;
    Ok(())
}

fn unpack_account_state<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<AccountState<()>, std::io::Error> {
    let counter = codec.get_u32()?;
    let delegation = unpack_delegation_type(codec)?;
    let value = codec.get_u64()?;
    let last_rewards = unpack_last_rewards(codec)?;
    Ok(AccountState {
        counter: SpendingCounter(counter),
        delegation,
        value: Value(value),
        last_rewards,
        extra: (),
    })
}

fn pack_delegation_ratio<W: std::io::Write>(
    delegation_ratio: &DelegationRatio,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u8(delegation_ratio.parts)?;
    // len of items in pools, for later use by the deserialize method
    codec.put_u64(delegation_ratio.pools.len() as u64)?;
    for (pool_id, u) in delegation_ratio.pools.iter() {
        codec.put_u8(*u)?;
        pack_poolid(pool_id, codec)?;
    }
    Ok(())
}

fn unpack_delegation_ratio<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<DelegationRatio, std::io::Error> {
    let parts = codec.get_u8()?;
    let pools_size = codec.get_u64()?;
    let mut pools: Vec<(PoolId, u8)> = Vec::with_capacity(pools_size as usize);
    for _ in 0..pools_size {
        let u = codec.get_u8()?;
        pools.push((unpack_poolid(codec)?, u));
    }
    match DelegationRatio::new(parts, pools) {
        Some(delegation_ratio) => Ok(delegation_ratio),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Error building DelegationRatio from serialized data",
        )),
    }
}

fn pack_delegation_type<W: std::io::Write>(
    delegation_type: &DelegationType,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    match delegation_type {
        DelegationType::NonDelegated => {
            codec.put_u8(0)?;
        }
        DelegationType::Full(pool_id) => {
            codec.put_u8(1)?;
            pack_poolid(pool_id, codec)?;
        }
        DelegationType::Ratio(delegation_ratio) => {
            codec.put_u8(2)?;
            pack_delegation_ration(delegation_ratio, codec)?;
        }
    }
    Ok(())
}

fn unpack_delegation_type<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<Self, std::io::Error> {
    match codec.get_u8()? {
        0 => Ok(DelegationType::NonDelegated),
        1 => {
            let pool_id = unpack_poolid(codec)?;
            Ok(DelegationType::Full(pool_id))
        }
        2 => {
            let delegation_ratio = unpack_delegation_ratio(codec)?;
            Ok(DelegationType::Ratio(delegation_ratio))
        }
        code => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid DelegationType type code {}", code),
        )),
    }
}

fn pack_last_rewards<W: std::io::Write>(
    last_rewards: &LastRewards,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u32(last_rewards.epoch)?;
    codec.put_u64(last_rewards.reward.0)?;
    Ok(())
}

fn unpack_last_rewards<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<LastRewards, std::io::Error> {
    Ok(LastRewards {
        epoch: codec.get_u32()?,
        reward: Value(codec.get_u64()?),
    })
}

fn pack_consensus_version<W: std::io::Write>(
    consensus_version: &ConsensusVersion,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    match consensus_version {
        ConsensusVersion::Bft => {
            codec.put_u8(1)?;
        }
        ConsensusVersion::GenesisPraos => {
            codec.put_u8(2)?;
        }
    }
    Ok(())
}

fn unpack_consensus_version<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<ConsensusVersion, std::io::Error> {
    match codec.get_u8()? {
        1 => Ok(ConsensusVersion::Bft),
        2 => Ok(ConsensusVersion::GenesisPraos),
        code => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Unrecognized code {} for ConsensusVersion", code),
        )),
    }
}

fn pack_pool_registration<W: std::io::Write>(
    pool_registration: &PoolRegistration,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    let byte_array = pool_registration.serialize();
    let bytes = byte_array.as_slice();
    let size = bytes.len() as u64;
    codec.put_u64(size)?;
    codec.put_bytes(bytes)?;
    Ok(())
}

fn unpack_pool_registration<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<PoolRegistration, std::io::Error> {
    let size = codec.get_u64()? as usize;
    let bytes_buff = codec.get_bytes(size)?;
    let mut read_buff = ReadBuf::from(&bytes_buff);
    match PoolRegistration::read(&mut read_buff) {
        Ok(res) => Ok(res),
        Err(err) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Error reading PoolRegistration data: {}", err),
        )),
    }
}

fn pack_config_param<W: Write>(
    config_param: &ConfigParam,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    let tag = Tag::from(config_param);
    let bytes = match config_param {
        ConfigParam::Block0Date(data) => data.to_payload(),
        ConfigParam::Discrimination(data) => data.to_payload(),
        ConfigParam::ConsensusVersion(data) => data.to_payload(),
        ConfigParam::SlotsPerEpoch(data) => data.to_payload(),
        ConfigParam::SlotDuration(data) => data.to_payload(),
        ConfigParam::EpochStabilityDepth(data) => data.to_payload(),
        ConfigParam::ConsensusGenesisPraosActiveSlotsCoeff(data) => data.to_payload(),
        ConfigParam::BlockContentMaxSize(data) => data.to_payload(),
        ConfigParam::AddBftLeader(data) => data.to_payload(),
        ConfigParam::RemoveBftLeader(data) => data.to_payload(),
        ConfigParam::LinearFee(data) => data.to_payload(),
        ConfigParam::ProposalExpiration(data) => data.to_payload(),
        ConfigParam::KESUpdateSpeed(data) => data.to_payload(),
        ConfigParam::TreasuryAdd(data) => data.to_payload(),
        ConfigParam::TreasuryParams(data) => data.to_payload(),
        ConfigParam::RewardPot(data) => data.to_payload(),
        ConfigParam::RewardParams(data) => data.to_payload(),
        ConfigParam::PerCertificateFees(data) => data.to_payload(),
        ConfigParam::FeesInTreasury(data) => data.to_payload(),
        ConfigParam::RewardLimitNone => Vec::with_capacity(0),
        ConfigParam::RewardLimitByAbsoluteStake(data) => data.to_payload(),
        ConfigParam::PoolRewardParticipationCapping(data) => data.to_payload(),
    };
    let taglen = TagLen::new(tag, bytes.len()).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "initial ent payload too big".to_string(),
        )
    })?;
    codec.put_u16(taglen.0)?;
    codec.write_all(&bytes)
}

fn unpack_config_param<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<ConfigParam, std::io::Error> {
    let tag_len = TagLen(codec.get_u16()?);
    let len = tag_len.get_len();
    let bytes = codec.get_bytes(len)?;
    // we will replicate the buffer so we can reuse the reader method
    let mut cursor = Cursor::new(Vec::with_capacity(2 + len));
    {
        let mut writer = Codec::new(&mut cursor);
        writer.put_u16(tag_len.0)?;
        writer.put_bytes(&bytes)?;
    }
    cursor.set_position(0);
    let mut read_buff = ReadBuf::from(cursor.get_ref());
    match ConfigParam::read(&mut read_buff) {
        Ok(res) => Ok(res),
        Err(err) => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Error reading ConfigParam: {}", err),
        )),
    }
}

fn pack_block_date<W: std::io::Write>(
    block_date: &BlockDate,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u32(block_date.epoch)?;
    codec.put_u32(block_date.slot_id)?;
    Ok(())
}

fn unpack_block_date<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<BlockDate, std::io::Error> {
    let epoch = codec.get_u32()?;
    let slot_id = codec.get_u32()?;
    Ok(BlockDate { epoch, slot_id })
}

fn pack_linear_fee<W: std::io::Write>(
    linear_fee: &LinearFee,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u64(linear_fee.constant)?;
    codec.put_u64(linear_fee.coefficient)?;
    codec.put_u64(linear_fee.certificate)?;
    pack_per_certificate_fee(&linear_fee.per_certificate_fees, codec)?;
    Ok(())
}

fn unpack_linear_fee<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<LinearFee, std::io::Error> {
    let constant = codec.get_u64()?;
    let coefficient = codec.get_u64()?;
    let certificate = codec.get_u64()?;
    let per_certificate_fees = unpack_per_certificate_fee(codec)?;
    Ok(LinearFee {
        constant,
        coefficient,
        certificate,
        per_certificate_fees,
    })
}

fn pack_per_certificate_fee<W: std::io::Write>(
    per_certificate_fee: &PerCertificateFee,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u64(
        per_certificate_fee
            .certificate_pool_registration
            .map(|v| v.get())
            .unwrap_or(0),
    )?;
    codec.put_u64(
        per_certificate_fee
            .certificate_stake_delegation
            .map(|v| v.get())
            .unwrap_or(0),
    )?;
    codec.put_u64(
        per_certificate_fee
            .certificate_owner_stake_delegation
            .map(|v| v.get())
            .unwrap_or(0),
    )?;
    Ok(())
}

fn unpack_per_certificate_fee<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<PerCertificateFee, std::io::Error> {
    let certificate_pool_registration = std::num::NonZeroU64::new(codec.get_u64()?);
    let certificate_stake_delegation = std::num::NonZeroU64::new(codec.get_u64()?);
    let certificate_owner_stake_delegation = std::num::NonZeroU64::new(codec.get_u64()?);

    Ok(PerCertificateFee {
        certificate_pool_registration,
        certificate_stake_delegation,
        certificate_owner_stake_delegation,
    })
}

fn pack_config_params<W: std::io::Write>(
    config_params: &ConfigParams,
    codec: &mut Codec<R>,
) -> Result<(), std::io::Error> {
    config_params.serialize(codec)
}

fn unpack_config_params<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<ConfigParams, std::io::Error> {
    ConfigParams::deserialize(codec)
}

fn pack_leader_id<W: std::io::Write>(
    leader_id: &LeaderId,
    codec: &mut Codec<R>,
) -> Result<(), std::io::Error> {
    serialize_public_key(&leader_id.0, codec)
}

fn unpack_leader_id<R: std::io::BufRead>(codec: &mut Codec<R>) -> Result<LeaderId, std::io::Error> {
    LeaderId::deserialize(codec)
}

fn pack_ledger_static_parameters<W: std::io::Write>(
    ledger_static_parameters: &LedgerStaticParameters,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    pack_header_id(ledger_static_parameters.block0_initial_hash, codec)?;
    codec.put_u64(ledger_static_parameters.block0_start_time.0)?;
    pack_discrimination(&ledger_static_parameters.discrimination, codec)?;
    codec.put_u32(ledger_static_parameters.kes_update_speed)?;
    Ok(())
}

fn unpack_ledger_static_parameters<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<LedgerStaticParameters, std::io::Error> {
    let block0_initial_hash = unpack_header_id(codec)?;
    let block0_start_time = config::Block0Date(codec.get_u64()?);
    let discrimination = unpack_discrimination(codec)?;
    let kes_update_speed = codec.get_u32()?;
    Ok(LedgerStaticParameters {
        block0_initial_hash,
        block0_start_time,
        discrimination,
        kes_update_speed,
    })
}

fn pack_globals<W: std::io::Write>(
    globals: &Globals,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    pack_date(&globals.date, codec)?;
    codec.put_u32(globals.chain_length.0)?;
    pack_ledger_static_parameters(&globals.static_params, codec)?;
    pack_time_era(&globals.era, codec)?;
    Ok(())
}

fn unpack_globals<R: std::io::BufRead>(codec: &mut Codec<R>) -> Result<Globals, std::io::Error> {
    let date = unpack_blockdate(codec)?;
    let chain_length = ChainLength(codec.get_u32()?);
    let static_params = unpack_ledger_static_parameters(codec)?;
    let era = unpack_timer_era(codec)?;
    Ok(Globals {
        date,
        chain_length,
        static_params,
        era,
    })
}

fn pack_pot_entry<W: std::io::Write>(
    entry: &pots::Entry,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    match entry {
        Entry::Fees(value) => {
            codec.put_u8(0)?;
            codec.put_u64(value.0)?;
        }
        Entry::Treasury(value) => {
            codec.put_u8(1)?;
            codec.put_u64(value.0)?;
        }
        Entry::Rewards(value) => {
            codec.put_u8(2)?;
            codec.put_u64(value.0)?;
        }
    }
    Ok(())
}

fn unpack_pot_entry<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<pots::Entry, std::io::Error> {
    match codec.get_u8()? {
        0 => Ok(Entry::Fees(Value(codec.get_u64()?))),
        1 => Ok(Entry::Treasury(Value(codec.get_u64()?))),
        2 => Ok(Entry::Rewards(Value(codec.get_u64()?))),
        code => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid Entry type code {}", code),
        )),
    }
}

fn pack_declaration_identifier<W: std::io::Write>(
    identifier: &multisig::Identifier,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    identifier.0.serialize(codec)
}

fn unpack_declaration_identifier<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<multisig::Identifier, std::io::Error> {
    Ok(multisig::Identifier(key::Hash::deserialize(codec)?))
}

fn pack_declaration<W: std::io::Write>(
    declaration: &Declaration,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u8(declaration.threshold)?;
    codec.put_u64(declaration.owners.len() as u64)?;
    for owner in &declaration.owners {
        pack_decl_element(owner, codec)?;
    }
    Ok(())
}

fn unpack_declaration<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<Declaration, std::io::Error> {
    let threshold = codec.get_u8()?;
    let size = codec.get_u64()?;
    let mut owners: Vec<DeclElement> = Vec::with_capacity(size as usize);
    for _ in 0..size {
        let decl_element = unpack_decl_element(codec)?;
        owners.push(decl_element);
    }
    Ok(Declaration { threshold, owners })
}

fn pack_decl_element<W: std::io::Write>(
    decl_element: &DeclElement,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    match &decl_element {
        DeclElement::Sub(declaration) => {
            codec.put_u8(0)?;
            pack_declaration(declaration, codec)?;
        }
        DeclElement::Owner(hash) => {
            codec.put_u8(1)?;
            hash.serialize(codec)?;
        }
    }
    Ok(())
}

fn unpack_decl_element<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<DeclElement, std::io::Error> {
    match codec.get_u8()? {
        0 => Ok(DeclElement::Sub(unpack_declaration(codec)?)),
        1 => Ok(DeclElement::Owner(key::Hash::deserialize(codec)?)),
        code => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid DeclElement type code {}", code),
        )),
    }
}

fn pack_pool_last_rewards<W: std::io::Write>(
    pool_last_rewards: &PoolLastRewards,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    codec.put_u32(pool_last_rewards.epoch)?;
    codec.put_u64(pool_last_rewards.value_taxed.0)?;
    codec.put_u64(pool_last_rewards.value_for_stakers.0)?;
    Ok(())
}

fn unpack_pool_last_rewards<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<PoolLastRewards, std::io::Error> {
    let epoch = codec.get_u32()?;
    let value_taxed = Value(codec.get_u64()?);
    let value_for_stakers = Value(codec.get_u64()?);

    Ok(PoolLastRewards {
        epoch,
        value_taxed,
        value_for_stakers,
    })
}

fn pack_pool_state<W: std::io::Write>(
    pool_state: &PoolState,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    pack_pool_last_rewards(&pool_state.last_rewards, codec)?;
    pack_registration(&pool_state.registration, codec)?;
    Ok(())
}

fn unpack_pool_state<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<PoolState, std::io::Error> {
    let last_rewards = unpack_pool_last_rewards(codec)?;
    let registration = Arc::new(unpack_pool_registration(codec)?);

    Ok(PoolState {
        last_rewards,
        registration,
    })
}

fn pack_update_proposal_state<W: std::io::Write>(
    update_proposal_state: &UpdateProposalState,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    pack_update_proposal(&update_proposal_state.proposal, codec)?;
    pack_block_date(&update_proposal_state.proposal_date, codec)?;
    codec.put_u64(update_proposal_state.votes.len() as u64)?;
    for e in &update_proposal_state.votes {
        e.serialize(codec)?;
    }
    Ok(())
}

fn unpack_update_proposal_state<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<UpdateProposalState, std::io::Error> {
    let proposal = unpack_update_proposal(codec)?;
    let proposal_date = unpack_block_date(codec)?;
    let total_votes = codec.get_u64()?;
    let mut votes: HashSet<UpdateVoterId> = HashSet::new();
    for _ in 0..total_votes {
        let id = UpdateVoterId::deserialize(codec)?;
        votes.insert(id);
    }
    Ok(UpdateProposalState {
        proposal,
        proposal_date,
        votes,
    })
}

fn pack_update_proposal<W: std::io::Write>(
    update_proposal: &UpdateProposal,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    update_proposal.serialize(codec)
}

fn unpack_update_proposal<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<UpdateProposal, std::io::Error> {
    UpdateProposal::deserialize(codec)
}

fn pack_update_proposal_id<W: std::io::Write>(
    update_proposal_id: &UpdateProposalId,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    update_proposal_id.serialize(codec)
}

fn unpack_update_proposal_id<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<UpdateProposalId, std::io::Error> {
    UpdateProposalId::deserialize(codec)
}

fn pack_utxo_entry<'a, Address, W: std::io::Write>(
    entry: &utxo::Entry<'a, Address>,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    entry.serialize(codec)
}

fn unpack_utxo_entry_owned<Address, R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<utxo::EntryOwned<Address>, std::io::Error> {
    utxo::EntryOwned::deserialize(codec)
}

#[derive(Debug, Eq, PartialEq)]
enum EntrySerializeCode {
    Globals = 0,
    Pot = 1,
    Utxo = 2,
    OldUtxo = 3,
    Account = 4,
    ConfigParam = 5,
    UpdateProposal = 6,
    MultisigAccount = 7,
    MultisigDeclaration = 8,
    StakePool = 9,
    LeaderParticipation = 10,
    SerializationEnd = 11,
}

impl EntrySerializeCode {
    pub fn from_u8(n: u8) -> Option<Self> {
        match n {
            0 => Some(EntrySerializeCode::Globals),
            1 => Some(EntrySerializeCode::Pot),
            2 => Some(EntrySerializeCode::Utxo),
            3 => Some(EntrySerializeCode::OldUtxo),
            4 => Some(EntrySerializeCode::Account),
            5 => Some(EntrySerializeCode::ConfigParam),
            6 => Some(EntrySerializeCode::UpdateProposal),
            7 => Some(EntrySerializeCode::MultisigAccount),
            8 => Some(EntrySerializeCode::MultisigDeclaration),
            9 => Some(EntrySerializeCode::StakePool),
            10 => Some(EntrySerializeCode::LeaderParticipation),
            11 => Some(EntrySerializeCode::SerializationEnd),
            _ => None,
        }
    }
}

fn pack_entry<W: std::io::Write>(
    entry: &Entry<'_>,
    codec: &mut Codec<W>,
) -> Result<(), std::io::Error> {
    match entry {
        Entry::Globals(entry) => {
            codec.put_u8(EntrySerializeCode::Globals as u8)?;
            pack_globals(entry, codec)?;
        }
        Entry::Pot(entry) => {
            codec.put_u8(EntrySerializeCode::Pot as u8)?;
            pack_pot_entry(entry, codec)?;
        }
        Entry::Utxo(entry) => {
            codec.put_u8(EntrySerializeCode::Utxo as u8)?;
            pack_utxo_entry(entry, codec)?;
        }
        Entry::OldUtxo(entry) => {
            codec.put_u8(EntrySerializeCode::OldUtxo as u8)?;
            pack_utxo_entry(entry, codec)?;
        }
        Entry::Account((identifier, account_state)) => {
            codec.put_u8(EntrySerializeCode::Account as u8)?;
            pack_account_identifier(identifier, codec)?;
            pack_account_state(account_state, codec)?;
        }
        Entry::ConfigParam(config_param) => {
            codec.put_u8(EntrySerializeCode::ConfigParam as u8)?;
            pack_config_param(config_param, codec)?;
        }
        Entry::UpdateProposal((proposal_id, proposal_state)) => {
            codec.put_u8(EntrySerializeCode::UpdateProposal as u8)?;
            pack_update_proposal_id(proposal_id, codec)?;
            pack_update_proposal_state(proposal_state, codec)?;
        }
        Entry::MultisigAccount((identifier, account_state)) => {
            codec.put_u8(EntrySerializeCode::MultisigAccount as u8)?;
            pack_declaration_identifier(identifier, codec)?;
            pack_account_state(account_state, codec)?;
        }
        Entry::MultisigDeclaration((identifier, declaration)) => {
            codec.put_u8(EntrySerializeCode::MultisigDeclaration as u8)?;
            pack_declaration_identifier(identifier, codec)?;
            pack_declaration(declaration, codec)?;
        }
        Entry::StakePool((pool_id, pool_state)) => {
            codec.put_u8(EntrySerializeCode::StakePool as u8)?;
            pack_digestof(pool_id.as_ref(), codec)?;
            pack_pool_state(pool_state, codec)?;
        }
        Entry::LeaderParticipation((pool_id, participation)) => {
            codec.put_u8(EntrySerializeCode::LeaderParticipation as u8)?;
            pack_digestof(pool_id.as_ref(), codec)?;
            codec.put_u32(**participation)?;
        }
    }
    Ok(())
}

fn unpack_entry_owned<R: std::io::BufRead>(
    codec: &mut Codec<R>,
) -> Result<EntryOwned, std::io::Error> {
    let code_u8 = codec.get_u8()?;
    let code = EntrySerializeCode::from_u8(code_u8).ok_or(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("Error reading Entry, not recognized type code {}", code_u8),
    ))?;
    match code {
        EntrySerializeCode::Globals => Ok(EntryOwned::Globals(unpack_globals(codec)?)),
        EntrySerializeCode::Pot => Ok(EntryOwned::Pot(unpack_pot_entry(codec)?)),
        EntrySerializeCode::Utxo => Ok(EntryOwned::Utxo(crate::utxo::EntryOwned::deserialize(
            codec,
        )?)),
        EntrySerializeCode::OldUtxo => Ok(EntryOwned::OldUtxo(
            crate::utxo::EntryOwned::deserialize(codec)?,
        )),
        EntrySerializeCode::Account => {
            let identifier = unpack_account_identifier(codec)?;
            let account = unpack_account_state(codec)?;
            Ok(EntryOwned::Account((identifier, account)))
        }
        EntrySerializeCode::ConfigParam => Ok(EntryOwned::ConfigParam(unpack_config_param(codec)?)),
        EntrySerializeCode::UpdateProposal => {
            let proposal_id = unpack_update_proposal_id(codec)?;
            let proposal_state = unpack_update_proposal_state(codec)?;
            Ok(EntryOwned::UpdateProposal((proposal_id, proposal_state)))
        }
        EntrySerializeCode::MultisigAccount => {
            let identifier = unpack_declaration_identifier(codec)?;
            let account_state = unpack_account_state(codec)?;
            Ok(EntryOwned::MultisigAccount((identifier, account_state)))
        }
        EntrySerializeCode::MultisigDeclaration => {
            let identifier = unpack_declaration_identifier(codec)?;
            let declaration = unpack_declaration(codec)?;
            Ok(EntryOwned::MultisigDeclaration((identifier, declaration)))
        }
        EntrySerializeCode::StakePool => {
            let pool_id = unpack_digestof(codec)?;
            let pool_state = unpack_pool_state(codec)?;
            Ok(EntryOwned::StakePool((pool_id, pool_state)))
        }
        EntrySerializeCode::LeaderParticipation => {
            let pool_id = unpack_digestof(codec)?;
            let v = codec.get_u32()?;
            Ok(EntryOwned::LeaderParticipation((pool_id, v)))
        }
        EntrySerializeCode::SerializationEnd => Ok(EntryOwned::StopEntry),
    }
}

impl Serialize for Ledger {
    type Error = std::io::Error;

    fn serialize<W: std::io::Write>(&self, writer: W) -> Result<(), Self::Error> {
        let mut codec = Codec::new(writer);
        for entry in self.iter() {
            pack_entry(&entry, &mut codec)?;
        }
        // Write finish flag
        codec.put_u8(EntrySerializeCode::SerializationEnd as u8)?;
        Ok(())
    }
}

// struct LazyLedgerDeserializer<'a> {
//     reader: &'a mut dyn std::io::BufRead,
// }
//
// impl<'a> LazyLedgerDeserializer<'a> {
//     fn new<R: std::io::BufRead>(reader: &'a mut R) -> LazyLedgerDeserializer<'a> {
//         Self {
//             reader,
//         }
//     }
//
//     fn next(&mut self) -> Option<EntryOwned> {
//         // TODO: What to do with an error here?
//         EntryOwned::deserialize(&mut self.reader).ok()
//     }
// }
//
// impl<'a> IntoIterator for &'a mut LazyLedgerDeserializer<'a> {
//     type Item = EntryOwned;
//     type IntoIter = LazyLedgerDeserializerIter<'a>;
//
//     fn into_iter(self) -> Self::IntoIter {
//         LazyLedgerDeserializerIter {
//             inner: self,
//         }
//     }
// }
//
// struct LazyLedgerDeserializerIter<'a> {
//     inner: &'a mut LazyLedgerDeserializer<'a>
// }
//
// impl<'a> Iterator for LazyLedgerDeserializerIter<'a> {
//     type Item = EntryOwned;
//
//     fn next(&mut self) -> Option<Self::Item> {
//         self.inner.next()
//     }
// }

// impl Deserialize for Ledger {
//     type Error = std::io::Error;
//
//     fn deserialize<R: std::io::BufRead>(reader: R) -> Result<Self, Self::Error> {
//         let mut codec = Codec::new(reader);
//         let res = Ok(Ledger::empty())::from_iter(
//             LazyLedgerDeserializer::new(&mut codec).into_iter().map(|entry| entry.to_entry())
//         );
//         match res {
//             Ok(ledger) => Ok(ledger),
//             Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{:?}", e))),
//         }
//     }
// }

#[cfg(any(test, feature = "property-test-api"))]
pub mod test {
    use super::*;
    use chain_crypto::{Blake2b256, Ed25519, KeyPair};
    use quickcheck::{quickcheck, Arbitrary, TestResult};
    use std::io::Cursor;
    use typed_bytes::{ByteArray, ByteSlice};

    #[test]
    pub fn addr_pack_unpack_bijection() -> Result<(), std::io::Error> {
        // set fake buffer
        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let mut codec = Codec::new(c);
        let address : Addr = "DdzFFzCqrhsqTG4t3uq5UBqFrxhxGVM6bvF4q1QcZXqUpizFddEEip7dx5rbife2s9o2fRU3hVKhRp4higog7As8z42s4AMw6Pcu8vL4".parse().unwrap();
        pack_addr(&address, &mut codec)?;
        c = codec.into_inner();
        // reset fake buffer
        c.set_position(0);
        codec = Codec::new(c);
        let new_address = unpack_addr(&mut codec)?;
        assert_eq!(address, new_address);
        Ok(())
    }

    #[test]
    pub fn discrimination_pack_unpack_bijection() -> Result<(), std::io::Error> {
        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let mut codec = Codec::new(c);
        pack_discrimination(&Discrimination::Test, &mut codec);
        pack_discrimination(&Discrimination::Production, &mut codec);

        c = codec.into_inner();
        c.set_position(0);
        codec = Codec::new(c);
        let test = unpack_discrimination(&mut codec)?;
        let production = unpack_discrimination(&mut codec)?;
        assert_eq!(Discrimination::Test, test);
        assert_eq!(Discrimination::Production, production);
        Ok(())
    }

    #[test]
    pub fn digestof_pack_unpack_bijection() -> Result<(), std::io::Error> {
        let data: [u8; 32] = [0u8; 32];
        let slice = &data[..];
        let byte_array: ByteArray<u8> = ByteArray::from(Vec::from(slice));
        let byte_slice: ByteSlice<u8> = byte_array.as_byteslice();
        let digest: DigestOf<Blake2b256, u8> = DigestOf::digest_byteslice(&byte_slice);

        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let mut codec = Codec::new(c);
        pack_digestof(&digest, &mut codec)?;
        c = codec.into_inner();
        c.set_position(0);
        codec = Codec::new(c);

        let deserialize_digest: DigestOf<Blake2b256, u8> = unpack_digestof(&mut codec)?;
        assert_eq!(digest, deserialize_digest);

        Ok(())
    }

    #[test]
    pub fn delegation_ratio_pack_unpack_bijection() -> Result<(), std::io::Error> {
        let fake_pool_id = StakePoolBuilder::new().build().id();
        let parts = 8u8;
        let pools: Vec<(PoolId, u8)> = vec![
            (fake_pool_id.clone(), 2u8),
            (fake_pool_id.clone(), 3u8),
            (fake_pool_id.clone(), 3u8),
        ];

        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let delegation_ratio = DelegationRatio::new(parts, pools).unwrap();
        delegation_ratio.serialize(&mut c)?;
        c.set_position(0);
        let deserialized_delegation_ratio = DelegationRatio::deserialize(&mut c)?;
        assert_eq!(delegation_ratio, deserialized_delegation_ratio);
        Ok(())
    }

    #[test]
    pub fn delegation_type_pack_unpack_bijection() -> Result<(), std::io::Error> {
        let fake_pool_id = StakePoolBuilder::new().build().id();

        let non_delegated = DelegationType::NonDelegated;
        let full = DelegationType::Full(fake_pool_id.clone());

        let parts = 8u8;
        let pools: Vec<(PoolId, u8)> = vec![
            (fake_pool_id.clone(), 2u8),
            (fake_pool_id.clone(), 3u8),
            (fake_pool_id.clone(), 3u8),
        ];
        let ratio = DelegationType::Ratio(DelegationRatio::new(parts, pools).unwrap());

        for delegation_type in [non_delegated, full, ratio].iter() {
            let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
            delegation_type.serialize(&mut c)?;
            c.set_position(0);
            let deserialized_delegation_type = DelegationType::deserialize(&mut c)?;
            assert_eq!(delegation_type, &deserialized_delegation_type);
        }
        Ok(())
    }

    #[test]
    pub fn account_state_pack_unpack_bijection() -> Result<(), std::io::Error> {
        let account_state = AccountState::new(Value(256), ());
        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        account_state.serialize(&mut c)?;
        c.set_position(0);
        let deserialized_account_state = AccountState::deserialize(&mut c)?;
        assert_eq!(account_state, deserialized_account_state);
        Ok(())
    }

    #[test]
    pub fn last_rewards_pack_unpack_bijection() -> Result<(), std::io::Error> {
        use std::io::Cursor;

        let last_rewards = LastRewards {
            epoch: 0,
            reward: Value(1),
        };

        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let mut codec = Codec::new(c);
        pack_last_rewards(&last_rewards, &mut codec)?;
        c = codec.into_inner();
        c.set_position(0);
        codec = Codec::new(c);
        let deserialize_last_rewards = unpack_last_rewards(&mut codec)?;
        assert_eq!(last_rewards, deserialize_last_rewards);
        Ok(())
    }

    #[test]
    fn pots_entry_pack_unpack_bijection() -> Result<(), std::io::Error> {
        use std::io::Cursor;
        for entry_value in [
            Entry::Fees(Value(10)),
            Entry::Rewards(Value(10)),
            Entry::Treasury(Value(10)),
        ]
        .iter()
        {
            let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
            let mut codec = Codec::new(c);
            pack_pot_entry(entry_value, &codec)?;
            c = codec.into_inner();
            c.set_position(0);
            codec = Codec::new(c);
            let other_value = unpack_pot_entry(&mut c)?;
            assert_eq!(entry_value, &other_value);
        }
        Ok(())
    }

    #[test]
    fn identifier_pack_unpack_bijection() -> Result<(), std::io::Error> {
        use std::io::Cursor;

        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
        let id_bytes: [u8; 32] = [0x1; 32];
        let identifier = Identifier::from(id_bytes);
        identifier.serialize(&mut c)?;
        c.set_position(0);
        let other_identifier = Identifier::deserialize(&mut c)?;
        assert_eq!(identifier, other_identifier);

        Ok(())
    }

    #[test]
    fn decl_element_pack_unpack_bijection() -> Result<(), std::io::Error> {
        use std::io::Cursor;

        let id_bytes: [u8; 32] = [0x1; 32];

        for decl_element in [
            DeclElement::Sub(Declaration {
                owners: Vec::new(),
                threshold: 10,
            }),
            DeclElement::Owner(key::Hash::from_bytes(id_bytes)),
        ]
        .iter()
        {
            let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());
            decl_element.serialize(&mut c)?;
            c.set_position(0);
            let other_value = DeclElement::deserialize(&mut c)?;
            assert_eq!(decl_element, &other_value);
        }
        Ok(())
    }

    #[test]
    fn declaration_pack_unpack_bijection() -> Result<(), std::io::Error> {
        use std::io::Cursor;

        let mut c: Cursor<Vec<u8>> = Cursor::new(Vec::new());

        let declaration = Declaration {
            owners: Vec::new(),
            threshold: 0,
        };

        declaration.serialize(&mut c)?;
        c.set_position(0);
        let other_value = Declaration::deserialize(&mut c)?;
        assert_eq!(declaration, other_value);

        Ok(())
    }

    quickcheck! {
        fn identifier_pack_unpack_bijection<i: Identifier>(id: Identifier) -> TestResult {
            unimplemented!()
        }

        fn consensus_version_serialization_bijection(v: ConsensusVersion) -> TestResult {
            unimplemented!()
        }

        fn pool_registration_serialize_deserialize_biyection(p: PoolRegistration) -> TestResult {
            unimplemented!()
        }

        fn config_param_pack_unpack_bijection(config_param: ConfigParam) -> TestResult {
            unimplemented!()
        }

        fn blockdate_pack_unpack_bijection(b: BlockDate) -> TestResult {
            unimplemented!()
        }

        fn per_certificate_fee_pack_unpack_bijection(b: PerCertificateFee) -> TestResult {
            unimplemented!()
        }

        fn linear_fee_pack_unpack_bijection(b: LinearFee) -> TestResult {
            unimplemented!()
        }

        fn leader_id_pack_unpack_biyection(leader_id: LeaderId) -> TestResult {
            unimplemented!()
        }

        fn globals_pack_unpack_bijection(b: Globals) -> TestResult {
            unimplemented!()
        }

        fn ledger_static_parameters_pack_unpack_bijection(b: LedgerStaticParameters) -> TestResult {
            unimplemented!()
        }

        fn pool_state_pack_unpack_bijection(b: PoolState) -> TestResult {
            unimplemented!()
        }

        fn pool_last_rewards_pack_unpack_bijection(b: PoolLastRewards) -> TestResult {
            unimplemented!()
        }

        fn update_proposal_state_pack_unpack_bijection(update_proposal_state: UpdateProposalState) -> TestResult {
            unimplemented!()
        }
    }

    #[test]
    #[ignore]
    fn ledger_serialize_deserialize_bijection() {
        unimplemented!()
    }
}
