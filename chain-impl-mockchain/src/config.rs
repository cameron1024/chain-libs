use crate::date::Epoch;
use crate::key::BftLeaderId;
use crate::milli::Milli;
use crate::rewards::{Ratio, TaxType};
use crate::value::Value;
use crate::{
    chaintypes::ConsensusType,
    fee::{LinearFee, PerCertificateFee, PerVoteCertificateFee},
    vote::CommitteeId,
};
use chain_addr::Discrimination;
use chain_core::mempack::{ReadBuf, ReadError, Readable};
use chain_core::packer::Codec;
use chain_core::property;
use chain_crypto::PublicKey;
#[cfg(feature = "evm")]
use chain_evm::machine::{BlockCoinBase, BlockHash, Config, Environment, Origin};
#[cfg(feature = "evm")]
use std::convert::TryInto;
use std::{
    fmt::{self, Display, Formatter},
    io::{self, Cursor, Write},
    num::{NonZeroU32, NonZeroU64},
};
use strum_macros::{AsRefStr, EnumIter, EnumString};
use typed_bytes::ByteBuilder;

/// Possible errors
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Error {
    InvalidTag,
    SizeInvalid,
    BoolInvalid,
    StructureInvalid,
    UnknownString(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Error::InvalidTag => write!(f, "Invalid config parameter tag"),
            Error::SizeInvalid => write!(f, "Invalid config parameter size"),
            Error::BoolInvalid => write!(f, "Invalid Boolean in config parameter"),
            Error::StructureInvalid => write!(f, "Invalid config parameter structure"),
            Error::UnknownString(s) => write!(f, "Invalid config parameter string '{}'", s),
        }
    }
}

impl std::error::Error for Error {}

impl From<ReadError> for Error {
    fn from(_: ReadError) -> Self {
        Error::StructureInvalid
    }
}

impl From<Error> for ReadError {
    fn from(error: Error) -> ReadError {
        ReadError::StructureInvalid(error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigParam {
    Block0Date(Block0Date),
    Discrimination(Discrimination),
    ConsensusVersion(ConsensusType),
    SlotsPerEpoch(u32),
    SlotDuration(u8),
    EpochStabilityDepth(u32),
    ConsensusGenesisPraosActiveSlotsCoeff(Milli),
    BlockContentMaxSize(u32),
    AddBftLeader(BftLeaderId),
    RemoveBftLeader(BftLeaderId),
    LinearFee(LinearFee),
    ProposalExpiration(u32),
    KesUpdateSpeed(u32),
    TreasuryAdd(Value),
    TreasuryParams(TaxType),
    RewardPot(Value),
    RewardParams(RewardParams),
    PerCertificateFees(PerCertificateFee),
    FeesInTreasury(bool),
    RewardLimitNone,
    RewardLimitByAbsoluteStake(Ratio),
    PoolRewardParticipationCapping((NonZeroU32, NonZeroU32)),
    AddCommitteeId(CommitteeId),
    RemoveCommitteeId(CommitteeId),
    PerVoteCertificateFees(PerVoteCertificateFee),
    TransactionMaxExpiryEpochs(u8),
    #[cfg(feature = "evm")]
    EvmParams(EvmConfigParams),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RewardParams {
    Linear {
        constant: u64,
        ratio: Ratio,
        epoch_start: Epoch,
        epoch_rate: NonZeroU32,
    },
    Halving {
        constant: u64,
        ratio: Ratio,
        epoch_start: Epoch,
        epoch_rate: NonZeroU32,
    },
}

#[cfg(feature = "evm")]
#[derive(Clone, Debug)]
/// EVM Configuration parameters needed for execution.
pub struct EvmConfigParams {
    /// EVM Block Configuration. It is boxed to reduce
    /// size difference when used in enum variants
    pub config: Box<Config>,
    /// EVM Block Environment.
    pub environment: Environment,
}

#[cfg(feature = "evm")]
impl Default for EvmConfigParams {
    fn default() -> Self {
        EvmConfigParams {
            config: Box::new(Config::istanbul()),
            environment: Environment {
                gas_price: Default::default(),
                origin: Default::default(),
                chain_id: Default::default(),
                block_hashes: Default::default(),
                block_number: Default::default(),
                block_coinbase: Default::default(),
                block_timestamp: Default::default(),
                block_difficulty: Default::default(),
                block_gas_limit: Default::default(),
            },
        }
    }
}

#[cfg(feature = "evm")]
impl Eq for EvmConfigParams {}

#[cfg(feature = "evm")]
impl PartialEq for EvmConfigParams {
    fn eq(&self, other: &Self) -> bool {
        fn compare_configs(a: &Config, b: &Config) -> bool {
            a.gas_ext_code == b.gas_ext_code
                && a.gas_ext_code_hash == b.gas_ext_code_hash
                && a.gas_sstore_set == b.gas_sstore_set
                && a.gas_sstore_reset == b.gas_sstore_reset
                && a.refund_sstore_clears == b.refund_sstore_clears
                && a.gas_balance == b.gas_balance
                && a.gas_sload == b.gas_sload
                && a.gas_suicide == b.gas_suicide
                && a.gas_suicide_new_account == b.gas_suicide_new_account
                && a.gas_call == b.gas_call
                && a.gas_expbyte == b.gas_expbyte
                && a.gas_transaction_create == b.gas_transaction_create
                && a.gas_transaction_call == b.gas_transaction_call
                && a.gas_transaction_zero_data == b.gas_transaction_zero_data
                && a.gas_transaction_non_zero_data == b.gas_transaction_non_zero_data
                && a.sstore_gas_metering == b.sstore_gas_metering
                && a.sstore_revert_under_stipend == b.sstore_revert_under_stipend
                && a.err_on_call_with_more_gas == b.err_on_call_with_more_gas
                && a.call_l64_after_gas == b.call_l64_after_gas
                && a.empty_considered_exists == b.empty_considered_exists
                && a.create_increase_nonce == b.create_increase_nonce
                && a.stack_limit == b.stack_limit
                && a.memory_limit == b.memory_limit
                && a.call_stack_limit == b.call_stack_limit
                && a.create_contract_limit == b.create_contract_limit
                && a.call_stipend == b.call_stipend
                && a.has_delegate_call == b.has_delegate_call
                && a.has_create2 == b.has_create2
                && a.has_revert == b.has_revert
                && a.has_return_data == b.has_return_data
                && a.has_bitwise_shifting == b.has_bitwise_shifting
                && a.has_chain_id == b.has_chain_id
                && a.has_self_balance == b.has_self_balance
                && a.has_ext_code_hash == b.has_ext_code_hash
                && a.estimate == b.estimate
        }
        compare_configs(&self.config, &other.config) && self.environment == other.environment
    }
}

// Discriminants can NEVER be 1024 or higher
#[derive(AsRefStr, Clone, Copy, Debug, EnumIter, EnumString, PartialEq)]
pub enum Tag {
    #[strum(to_string = "discrimination")]
    Discrimination = 1,
    #[strum(to_string = "block0-date")]
    Block0Date = 2,
    #[strum(to_string = "block0-consensus")]
    ConsensusVersion = 3,
    #[strum(to_string = "slots-per-epoch")]
    SlotsPerEpoch = 4,
    #[strum(to_string = "slot-duration")]
    SlotDuration = 5,
    #[strum(to_string = "epoch-stability-depth")]
    EpochStabilityDepth = 6,
    #[strum(to_string = "genesis-praos-param-f")]
    ConsensusGenesisPraosActiveSlotsCoeff = 8,
    #[strum(to_string = "block-content-max-size")]
    BlockContentMaxSize = 9,
    #[strum(to_string = "add-bft-leader")]
    AddBftLeader = 11,
    #[strum(to_string = "remove-bft-leader")]
    RemoveBftLeader = 12,
    #[strum(to_string = "linear-fee")]
    LinearFee = 14,
    #[strum(to_string = "proposal-expiration")]
    ProposalExpiration = 15,
    #[strum(to_string = "kes-update-speed")]
    KesUpdateSpeed = 16,
    #[strum(to_string = "treasury")]
    TreasuryAdd = 17,
    #[strum(to_string = "treasury-params")]
    TreasuryParams = 18,
    #[strum(to_string = "reward-pot")]
    RewardPot = 19,
    #[strum(to_string = "reward-params")]
    RewardParams = 20,
    #[strum(to_string = "per-certificate-fees")]
    PerCertificateFees = 21,
    #[strum(to_string = "fees-in-treasury")]
    FeesInTreasury = 22,
    #[strum(to_string = "reward-limit-none")]
    RewardLimitNone = 23,
    #[strum(to_string = "reward-limit-by-absolute-stake")]
    RewardLimitByAbsoluteStake = 24,
    #[strum(to_string = "pool-reward-participation-capping")]
    PoolRewardParticipationCapping = 25,
    #[strum(to_string = "add-committee-id")]
    AddCommitteeId = 26,
    #[strum(to_string = "remove-committee-id")]
    RemoveCommitteeId = 27,
    #[strum(to_string = "per-vote-certificate-fees")]
    PerVoteCertificateFees = 28,
    #[strum(to_string = "transaction-maximum-expiry-epochs")]
    TransactionMaxExpiryEpochs = 29,
    #[cfg(feature = "evm")]
    #[strum(to_string = "evm-config-params")]
    EvmParams = 30,
}

impl Tag {
    pub fn from_u16(v: u16) -> Option<Self> {
        match v {
            1 => Some(Tag::Discrimination),
            2 => Some(Tag::Block0Date),
            3 => Some(Tag::ConsensusVersion),
            4 => Some(Tag::SlotsPerEpoch),
            5 => Some(Tag::SlotDuration),
            6 => Some(Tag::EpochStabilityDepth),
            8 => Some(Tag::ConsensusGenesisPraosActiveSlotsCoeff),
            9 => Some(Tag::BlockContentMaxSize),
            11 => Some(Tag::AddBftLeader),
            12 => Some(Tag::RemoveBftLeader),
            14 => Some(Tag::LinearFee),
            15 => Some(Tag::ProposalExpiration),
            16 => Some(Tag::KesUpdateSpeed),
            17 => Some(Tag::TreasuryAdd),
            18 => Some(Tag::TreasuryParams),
            19 => Some(Tag::RewardPot),
            20 => Some(Tag::RewardParams),
            21 => Some(Tag::PerCertificateFees),
            22 => Some(Tag::FeesInTreasury),
            23 => Some(Tag::RewardLimitNone),
            24 => Some(Tag::RewardLimitByAbsoluteStake),
            25 => Some(Tag::PoolRewardParticipationCapping),
            26 => Some(Tag::AddCommitteeId),
            27 => Some(Tag::RemoveCommitteeId),
            28 => Some(Tag::PerVoteCertificateFees),
            29 => Some(Tag::TransactionMaxExpiryEpochs),
            #[cfg(feature = "evm")]
            30 => Some(Tag::EvmParams),
            _ => None,
        }
    }
}

impl<'a> From<&'a ConfigParam> for Tag {
    fn from(config_param: &'a ConfigParam) -> Self {
        match config_param {
            ConfigParam::Block0Date(_) => Tag::Block0Date,
            ConfigParam::Discrimination(_) => Tag::Discrimination,
            ConfigParam::ConsensusVersion(_) => Tag::ConsensusVersion,
            ConfigParam::SlotsPerEpoch(_) => Tag::SlotsPerEpoch,
            ConfigParam::SlotDuration(_) => Tag::SlotDuration,
            ConfigParam::EpochStabilityDepth(_) => Tag::EpochStabilityDepth,
            ConfigParam::ConsensusGenesisPraosActiveSlotsCoeff(_) => {
                Tag::ConsensusGenesisPraosActiveSlotsCoeff
            }
            ConfigParam::BlockContentMaxSize(_) => Tag::BlockContentMaxSize,
            ConfigParam::AddBftLeader(_) => Tag::AddBftLeader,
            ConfigParam::RemoveBftLeader(_) => Tag::RemoveBftLeader,
            ConfigParam::LinearFee(_) => Tag::LinearFee,
            ConfigParam::ProposalExpiration(_) => Tag::ProposalExpiration,
            ConfigParam::KesUpdateSpeed(_) => Tag::KesUpdateSpeed,
            ConfigParam::TreasuryAdd(_) => Tag::TreasuryAdd,
            ConfigParam::TreasuryParams(_) => Tag::TreasuryParams,
            ConfigParam::RewardPot(_) => Tag::RewardPot,
            ConfigParam::RewardParams(_) => Tag::RewardParams,
            ConfigParam::PerCertificateFees(_) => Tag::PerCertificateFees,
            ConfigParam::FeesInTreasury(_) => Tag::FeesInTreasury,
            ConfigParam::RewardLimitNone => Tag::RewardLimitNone,
            ConfigParam::RewardLimitByAbsoluteStake(_) => Tag::RewardLimitByAbsoluteStake,
            ConfigParam::PoolRewardParticipationCapping(..) => Tag::PoolRewardParticipationCapping,
            ConfigParam::AddCommitteeId(..) => Tag::AddCommitteeId,
            ConfigParam::RemoveCommitteeId(..) => Tag::RemoveCommitteeId,
            ConfigParam::PerVoteCertificateFees(..) => Tag::PerVoteCertificateFees,
            ConfigParam::TransactionMaxExpiryEpochs(..) => Tag::TransactionMaxExpiryEpochs,
            #[cfg(feature = "evm")]
            ConfigParam::EvmParams(_) => Tag::EvmParams,
        }
    }
}

impl Readable for ConfigParam {
    fn read(buf: &mut ReadBuf) -> Result<Self, ReadError> {
        let taglen = TagLen(buf.get_u16()?);
        let bytes = buf.get_slice(taglen.get_len())?;
        match taglen.get_tag()? {
            Tag::Block0Date => ConfigParamVariant::from_payload(bytes).map(ConfigParam::Block0Date),
            Tag::Discrimination => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::Discrimination)
            }
            Tag::ConsensusVersion => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::ConsensusVersion)
            }
            Tag::SlotsPerEpoch => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::SlotsPerEpoch)
            }
            Tag::SlotDuration => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::SlotDuration)
            }
            Tag::EpochStabilityDepth => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::EpochStabilityDepth)
            }
            Tag::ConsensusGenesisPraosActiveSlotsCoeff => ConfigParamVariant::from_payload(bytes)
                .map(ConfigParam::ConsensusGenesisPraosActiveSlotsCoeff),
            Tag::BlockContentMaxSize => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::BlockContentMaxSize)
            }
            Tag::AddBftLeader => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::AddBftLeader)
            }
            Tag::RemoveBftLeader => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::RemoveBftLeader)
            }
            Tag::LinearFee => ConfigParamVariant::from_payload(bytes).map(ConfigParam::LinearFee),
            Tag::ProposalExpiration => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::ProposalExpiration)
            }
            Tag::KesUpdateSpeed => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::KesUpdateSpeed)
            }
            Tag::TreasuryAdd => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::TreasuryAdd)
            }
            Tag::TreasuryParams => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::TreasuryParams)
            }
            Tag::RewardPot => ConfigParamVariant::from_payload(bytes).map(ConfigParam::RewardPot),
            Tag::RewardParams => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::RewardParams)
            }
            Tag::PerCertificateFees => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::PerCertificateFees)
            }
            Tag::FeesInTreasury => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::FeesInTreasury)
            }
            Tag::RewardLimitNone => {
                if !bytes.is_empty() {
                    Err(Error::SizeInvalid)
                } else {
                    Ok(ConfigParam::RewardLimitNone)
                }
            }
            Tag::RewardLimitByAbsoluteStake => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::RewardLimitByAbsoluteStake)
            }
            Tag::PoolRewardParticipationCapping => ConfigParamVariant::from_payload(bytes)
                .map(ConfigParam::PoolRewardParticipationCapping),
            Tag::AddCommitteeId => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::AddCommitteeId)
            }
            Tag::RemoveCommitteeId => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::RemoveCommitteeId)
            }
            Tag::PerVoteCertificateFees => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::PerVoteCertificateFees)
            }
            Tag::TransactionMaxExpiryEpochs => {
                ConfigParamVariant::from_payload(bytes).map(ConfigParam::TransactionMaxExpiryEpochs)
            }
            #[cfg(feature = "evm")]
            Tag::EvmParams => ConfigParamVariant::from_payload(bytes).map(ConfigParam::EvmParams),
        }
        .map_err(Into::into)
    }
}

impl property::Serialize for ConfigParam {
    type Error = io::Error;

    fn serialize<W: Write>(&self, writer: W) -> Result<(), Self::Error> {
        let tag = Tag::from(self);
        let bytes = match self {
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
            ConfigParam::KesUpdateSpeed(data) => data.to_payload(),
            ConfigParam::TreasuryAdd(data) => data.to_payload(),
            ConfigParam::TreasuryParams(data) => data.to_payload(),
            ConfigParam::RewardPot(data) => data.to_payload(),
            ConfigParam::RewardParams(data) => data.to_payload(),
            ConfigParam::PerCertificateFees(data) => data.to_payload(),
            ConfigParam::FeesInTreasury(data) => data.to_payload(),
            ConfigParam::RewardLimitNone => Vec::with_capacity(0),
            ConfigParam::RewardLimitByAbsoluteStake(data) => data.to_payload(),
            ConfigParam::PoolRewardParticipationCapping(data) => data.to_payload(),
            ConfigParam::AddCommitteeId(data) => data.to_payload(),
            ConfigParam::RemoveCommitteeId(data) => data.to_payload(),
            ConfigParam::PerVoteCertificateFees(data) => data.to_payload(),
            ConfigParam::TransactionMaxExpiryEpochs(data) => data.to_payload(),
            #[cfg(feature = "evm")]
            ConfigParam::EvmParams(data) => data.to_payload(),
        };
        let taglen = TagLen::new(tag, bytes.len()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "initial ent payload too big".to_string(),
            )
        })?;
        let mut codec = Codec::new(writer);
        codec.put_u16(taglen.0)?;
        codec.write_all(&bytes)
    }
}

impl property::Deserialize for ConfigParam {
    type Error = std::io::Error;

    fn deserialize<R: std::io::BufRead>(reader: R) -> Result<Self, Self::Error> {
        let mut codec = Codec::new(reader);
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
}

trait ConfigParamVariant: Clone + Eq + PartialEq {
    fn to_payload(&self) -> Vec<u8>;
    fn from_payload(payload: &[u8]) -> Result<Self, Error>;
}

/// Seconds elapsed since 1-Jan-1970 (unix time)
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct Block0Date(pub u64);

impl ConfigParamVariant for Block0Date {
    fn to_payload(&self) -> Vec<u8> {
        self.0.to_payload()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        u64::from_payload(payload).map(Block0Date)
    }
}

impl ConfigParamVariant for TaxType {
    fn to_payload(&self) -> Vec<u8> {
        self.serialize_in(ByteBuilder::new()).finalize_as_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut rb = ReadBuf::from(payload);
        let tax_type = TaxType::read_frombuf(&mut rb)?;
        rb.expect_end()?;
        Ok(tax_type)
    }
}

impl ConfigParamVariant for Ratio {
    fn to_payload(&self) -> Vec<u8> {
        let bb: ByteBuilder<Ratio> = ByteBuilder::new();
        bb.u64(self.numerator)
            .u64(self.denominator.get())
            .finalize_as_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut rb = ReadBuf::from(payload);
        let num = rb.get_u64()?;
        let denom = rb.get_nz_u64()?;
        rb.expect_end()?;
        Ok(Ratio {
            numerator: num,
            denominator: denom,
        })
    }
}
impl ConfigParamVariant for RewardParams {
    fn to_payload(&self) -> Vec<u8> {
        let bb: ByteBuilder<RewardParams> = match self {
            RewardParams::Linear {
                constant,
                ratio,
                epoch_start,
                epoch_rate,
            } => ByteBuilder::new()
                .u8(1)
                .u64(*constant)
                .u64(ratio.numerator)
                .u64(ratio.denominator.get())
                .u32(*epoch_start)
                .u32(epoch_rate.get()),
            RewardParams::Halving {
                constant,
                ratio,
                epoch_start,
                epoch_rate,
            } => ByteBuilder::new()
                .u8(2)
                .u64(*constant)
                .u64(ratio.numerator)
                .u64(ratio.denominator.get())
                .u32(*epoch_start)
                .u32(epoch_rate.get()),
        };
        bb.finalize_as_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut rb = ReadBuf::from(payload);
        match rb.get_u8()? {
            1 => {
                let start = rb.get_u64()?;
                let num = rb.get_u64()?;
                let denom = rb.get_nz_u64()?;
                let estart = rb.get_u32()?;
                let erate = rb.get_nz_u32()?;
                rb.expect_end()?;
                Ok(RewardParams::Linear {
                    constant: start,
                    ratio: Ratio {
                        numerator: num,
                        denominator: denom,
                    },
                    epoch_start: estart,
                    epoch_rate: erate,
                })
            }
            2 => {
                let start = rb.get_u64()?;
                let num = rb.get_u64()?;
                let denom = rb.get_nz_u64()?;
                let estart = rb.get_u32()?;
                let erate = rb.get_nz_u32()?;
                rb.expect_end()?;
                Ok(RewardParams::Halving {
                    constant: start,
                    ratio: Ratio {
                        numerator: num,
                        denominator: denom,
                    },
                    epoch_start: estart,
                    epoch_rate: erate,
                })
            }
            _ => Err(Error::InvalidTag),
        }
    }
}

impl ConfigParamVariant for Value {
    fn to_payload(&self) -> Vec<u8> {
        self.0.to_payload()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        u64::from_payload(payload).map(Value)
    }
}

const VAL_PROD: u8 = 1;
const VAL_TEST: u8 = 2;

impl ConfigParamVariant for Discrimination {
    fn to_payload(&self) -> Vec<u8> {
        match self {
            Discrimination::Production => vec![VAL_PROD],
            Discrimination::Test => vec![VAL_TEST],
        }
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        if payload.len() != 1 {
            return Err(Error::SizeInvalid);
        };
        match payload[0] {
            VAL_PROD => Ok(Discrimination::Production),
            VAL_TEST => Ok(Discrimination::Test),
            _ => Err(Error::StructureInvalid),
        }
    }
}

impl ConfigParamVariant for ConsensusType {
    fn to_payload(&self) -> Vec<u8> {
        (*self as u16).to_be_bytes().to_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut bytes = 0u16.to_ne_bytes();
        if payload.len() != bytes.len() {
            return Err(Error::SizeInvalid);
        };
        bytes.copy_from_slice(payload);
        let integer = u16::from_be_bytes(bytes);
        ConsensusType::from_u16(integer).ok_or(Error::StructureInvalid)
    }
}

impl ConfigParamVariant for BftLeaderId {
    fn to_payload(&self) -> Vec<u8> {
        self.as_ref().to_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        PublicKey::from_binary(payload)
            .map(Into::into)
            .map_err(|_| Error::SizeInvalid)
    }
}

impl ConfigParamVariant for bool {
    fn to_payload(&self) -> Vec<u8> {
        vec![if *self { 1 } else { 0 }]
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        match payload.len() {
            1 => match payload[0] {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(Error::BoolInvalid),
            },
            _ => Err(Error::SizeInvalid),
        }
    }
}

impl ConfigParamVariant for u8 {
    fn to_payload(&self) -> Vec<u8> {
        vec![*self]
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        match payload.len() {
            1 => Ok(payload[0]),
            _ => Err(Error::SizeInvalid),
        }
    }
}

impl ConfigParamVariant for u64 {
    fn to_payload(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut bytes = Self::default().to_ne_bytes();
        if payload.len() != bytes.len() {
            return Err(Error::SizeInvalid);
        };
        bytes.copy_from_slice(payload);
        Ok(Self::from_be_bytes(bytes))
    }
}

impl ConfigParamVariant for u32 {
    fn to_payload(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut bytes = Self::default().to_ne_bytes();
        if payload.len() != bytes.len() {
            return Err(Error::SizeInvalid);
        };
        bytes.copy_from_slice(payload);
        Ok(Self::from_be_bytes(bytes))
    }
}

impl ConfigParamVariant for (NonZeroU32, NonZeroU32) {
    fn to_payload(&self) -> Vec<u8> {
        let bb: ByteBuilder<()> = ByteBuilder::new();
        bb.u32(self.0.get()).u32(self.1.get()).finalize_as_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut rb = ReadBuf::from(payload);
        let x = rb.get_nz_u32()?;
        let y = rb.get_nz_u32()?;
        Ok((x, y))
    }
}

impl ConfigParamVariant for Milli {
    fn to_payload(&self) -> Vec<u8> {
        self.to_millis().to_payload()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        u64::from_payload(payload).map(Milli::from_millis)
    }
}

impl ConfigParamVariant for LinearFee {
    fn to_payload(&self) -> Vec<u8> {
        let mut v = self.constant.to_payload();
        v.extend(self.coefficient.to_payload());
        v.extend(self.certificate.to_payload());
        v
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        if payload.len() != 3 * 8 {
            return Err(Error::SizeInvalid);
        }
        Ok(LinearFee {
            constant: u64::from_payload(&payload[0..8])?,
            coefficient: u64::from_payload(&payload[8..16])?,
            certificate: u64::from_payload(&payload[16..24])?,
            per_certificate_fees: PerCertificateFee::default(),
            per_vote_certificate_fees: PerVoteCertificateFee::default(),
        })
    }
}

impl ConfigParamVariant for PerCertificateFee {
    fn to_payload(&self) -> Vec<u8> {
        let mut v = self
            .certificate_pool_registration
            .map(|v| v.get())
            .unwrap_or(0)
            .to_payload();
        v.extend(
            self.certificate_stake_delegation
                .map(|v| v.get())
                .unwrap_or(0)
                .to_payload(),
        );
        v.extend(
            self.certificate_owner_stake_delegation
                .map(|v| v.get())
                .unwrap_or(0)
                .to_payload(),
        );
        v
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        if payload.len() != 3 * 8 {
            return Err(Error::SizeInvalid);
        }
        Ok(PerCertificateFee {
            certificate_pool_registration: NonZeroU64::new(u64::from_payload(&payload[0..8])?),
            certificate_stake_delegation: NonZeroU64::new(u64::from_payload(&payload[8..16])?),
            certificate_owner_stake_delegation: NonZeroU64::new(u64::from_payload(
                &payload[16..24],
            )?),
        })
    }
}

impl ConfigParamVariant for PerVoteCertificateFee {
    fn to_payload(&self) -> Vec<u8> {
        let mut v = self
            .certificate_vote_plan
            .map(|v| v.get())
            .unwrap_or(0)
            .to_payload();
        v.extend(
            self.certificate_vote_cast
                .map(|v| v.get())
                .unwrap_or(0)
                .to_payload(),
        );
        v
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        if payload.len() != 2 * 8 {
            return Err(Error::SizeInvalid);
        }
        Ok(Self {
            certificate_vote_plan: NonZeroU64::new(u64::from_payload(&payload[0..8])?),
            certificate_vote_cast: NonZeroU64::new(u64::from_payload(&payload[8..16])?),
        })
    }
}

impl ConfigParamVariant for CommitteeId {
    fn to_payload(&self) -> Vec<u8> {
        self.as_ref().to_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        use std::convert::TryFrom as _;
        Self::try_from(payload).map_err(|err| Error::UnknownString(err.to_string()))
    }
}

#[cfg(feature = "evm")]
impl ConfigParamVariant for EvmConfigParams {
    fn to_payload(&self) -> Vec<u8> {
        let bb: ByteBuilder<EvmConfigParams> = ByteBuilder::new()
            .u64(self.config.gas_ext_code)
            .u64(self.config.gas_ext_code_hash)
            .u64(self.config.gas_sstore_set)
            .u64(self.config.gas_sstore_reset)
            .u64(self.config.refund_sstore_clears.try_into().unwrap())
            .u64(self.config.gas_balance)
            .u64(self.config.gas_sload)
            .u64(self.config.gas_suicide)
            .u64(self.config.gas_suicide_new_account)
            .u64(self.config.gas_call)
            .u64(self.config.gas_expbyte)
            .u64(self.config.gas_transaction_create)
            .u64(self.config.gas_transaction_call)
            .u64(self.config.gas_transaction_zero_data)
            .u64(self.config.gas_transaction_non_zero_data)
            .u8(self.config.sstore_gas_metering as u8)
            .u8(self.config.sstore_revert_under_stipend as u8)
            .u8(self.config.err_on_call_with_more_gas as u8)
            .u8(self.config.call_l64_after_gas as u8)
            .u8(self.config.empty_considered_exists as u8)
            .u8(self.config.create_increase_nonce as u8)
            .u64(self.config.stack_limit as u64)
            .u64(self.config.memory_limit as u64)
            .u64(self.config.call_stack_limit as u64);
        let bb = if let Some(limit) = self.config.create_contract_limit {
            bb.u8(1).u64(limit as u64)
        } else {
            bb.u8(0)
        };
        let bb = bb
            .u64(self.config.call_stipend as u64)
            .u8(self.config.has_delegate_call as u8)
            .u8(self.config.has_create2 as u8)
            .u8(self.config.has_revert as u8)
            .u8(self.config.has_return_data as u8)
            .u8(self.config.has_bitwise_shifting as u8)
            .u8(self.config.has_chain_id as u8)
            .u8(self.config.has_self_balance as u8)
            .u8(self.config.has_ext_code_hash as u8)
            .u8(self.config.estimate as u8);
        let bb = bb
            .bytes(&<[u8; 32]>::from(self.environment.gas_price))
            .bytes(self.environment.origin.as_fixed_bytes())
            .bytes(&<[u8; 32]>::from(self.environment.chain_id));
        let bb = if self.environment.block_hashes.is_empty() {
            bb.u64(0)
        } else {
            let bb = bb.u64(self.environment.block_hashes.len().try_into().unwrap());
            self.environment
                .block_hashes
                .iter()
                .fold(bb, |b, h| b.bytes(h.as_fixed_bytes()))
        };
        let bb = bb
            .bytes(&<[u8; 32]>::from(self.environment.block_number))
            .bytes(self.environment.block_coinbase.as_fixed_bytes())
            .bytes(&<[u8; 32]>::from(self.environment.block_timestamp))
            .bytes(&<[u8; 32]>::from(self.environment.block_difficulty))
            .bytes(&<[u8; 32]>::from(self.environment.block_gas_limit));
        bb.finalize_as_vec()
    }

    fn from_payload(payload: &[u8]) -> Result<Self, Error> {
        let mut rb = ReadBuf::from(payload);
        // Read Config
        let gas_ext_code = rb.get_u64()?;
        let gas_ext_code_hash = rb.get_u64()?;
        let gas_sstore_set = rb.get_u64()?;
        let gas_sstore_reset = rb.get_u64()?;
        let refund_sstore_clears: i64 = rb.get_u64()?.try_into().unwrap();
        let gas_balance = rb.get_u64()?;
        let gas_sload = rb.get_u64()?;
        let gas_suicide = rb.get_u64()?;
        let gas_suicide_new_account = rb.get_u64()?;
        let gas_call = rb.get_u64()?;
        let gas_expbyte = rb.get_u64()?;
        let gas_transaction_create = rb.get_u64()?;
        let gas_transaction_call = rb.get_u64()?;
        let gas_transaction_zero_data = rb.get_u64()?;
        let gas_transaction_non_zero_data = rb.get_u64()?;
        let sstore_gas_metering = rb.get_u8()? != 0;
        let sstore_revert_under_stipend = rb.get_u8()? != 0;
        let err_on_call_with_more_gas = rb.get_u8()? != 0;
        let call_l64_after_gas = rb.get_u8()? != 0;
        let empty_considered_exists = rb.get_u8()? != 0;
        let create_increase_nonce = rb.get_u8()? != 0;
        let stack_limit = rb.get_u64()? as usize;
        let memory_limit = rb.get_u64()? as usize;
        let call_stack_limit = rb.get_u64()? as usize;

        // Check if create contract limit is set
        let create_contract_limit = if rb.get_u8()? != 0 {
            Some(rb.get_u64()? as usize)
        } else {
            None
        };
        let call_stipend = rb.get_u64()?;
        let has_delegate_call = rb.get_u8()? != 0;
        let has_create2 = rb.get_u8()? != 0;
        let has_revert = rb.get_u8()? != 0;
        let has_return_data = rb.get_u8()? != 0;
        let has_bitwise_shifting = rb.get_u8()? != 0;
        let has_chain_id = rb.get_u8()? != 0;
        let has_self_balance = rb.get_u8()? != 0;
        let has_ext_code_hash = rb.get_u8()? != 0;
        let estimate = rb.get_u8()? != 0;

        let config = Config {
            gas_ext_code,
            gas_ext_code_hash,
            gas_sstore_set,
            gas_sstore_reset,
            refund_sstore_clears,
            gas_balance,
            gas_sload,
            gas_suicide,
            gas_suicide_new_account,
            gas_call,
            gas_expbyte,
            gas_transaction_create,
            gas_transaction_call,
            gas_transaction_zero_data,
            gas_transaction_non_zero_data,
            sstore_gas_metering,
            sstore_revert_under_stipend,
            err_on_call_with_more_gas,
            call_l64_after_gas,
            empty_considered_exists,
            create_increase_nonce,
            stack_limit,
            memory_limit,
            call_stack_limit,
            create_contract_limit,
            call_stipend,
            has_delegate_call,
            has_create2,
            has_revert,
            has_return_data,
            has_bitwise_shifting,
            has_chain_id,
            has_self_balance,
            has_ext_code_hash,
            estimate,
        };

        // Read Enviroment
        let gas_price = rb.get_slice(32)?.into();
        let origin = Origin::from_slice(rb.get_slice(20)?);
        let chain_id = rb.get_slice(32)?.into();

        let block_hashes = match rb.get_u64()? {
            0 => Vec::new(),
            n => (0..n)
                .map(|_| BlockHash::from_slice(rb.get_slice(32).unwrap()))
                .collect(),
        };
        let block_number = rb.get_slice(32)?.into();
        let block_coinbase = BlockCoinBase::from_slice(rb.get_slice(20)?);
        let block_timestamp = rb.get_slice(32)?.into();
        let block_difficulty = rb.get_slice(32)?.into();
        let block_gas_limit = rb.get_slice(32)?.into();

        let environment = Environment {
            gas_price,
            origin,
            chain_id,
            block_hashes,
            block_number,
            block_coinbase,
            block_timestamp,
            block_difficulty,
            block_gas_limit,
        };

        Ok(EvmConfigParams {
            config: Box::new(config),
            environment,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TagLen(u16);

const MAXIMUM_LEN: usize = 64;

impl TagLen {
    pub fn new(tag: Tag, len: usize) -> Option<Self> {
        if len < MAXIMUM_LEN {
            Some(TagLen((tag as u16) << 6 | len as u16))
        } else {
            None
        }
    }

    pub fn get_len(self) -> usize {
        (self.0 & 0b11_1111) as usize
    }

    pub fn get_tag(self) -> Result<Tag, Error> {
        Tag::from_u16(self.0 >> 6).ok_or(Error::InvalidTag)
    }
}

#[cfg(any(test, feature = "property-test-api"))]
mod test {
    use super::*;
    #[cfg(test)]
    use quickcheck::TestResult;
    use quickcheck::{Arbitrary, Gen};
    use strum::IntoEnumIterator;

    quickcheck! {
        fn tag_len_computation_correct(tag: Tag, len: usize) -> TestResult {
            let len = len % MAXIMUM_LEN;
            let tag_len = TagLen::new(tag, len).unwrap();

            assert_eq!(Ok(tag), tag_len.get_tag(), "Invalid tag");
            assert_eq!(len, tag_len.get_len(), "Invalid len");
            TestResult::passed()
        }

        fn linear_fee_to_payload_from_payload(fee: LinearFee) -> TestResult {
            let payload = fee.to_payload();
            let decoded = LinearFee::from_payload(&payload).unwrap();

            TestResult::from_bool(fee == decoded)
        }

        fn per_certificate_fee_to_payload_from_payload(fee: PerCertificateFee) -> TestResult {
            let payload = fee.to_payload();
            let decoded = PerCertificateFee::from_payload(&payload).unwrap();

            TestResult::from_bool(fee == decoded)
        }

        fn config_param_serialize_correct(param: ConfigParam) -> bool {
            use chain_core::property::{Serialize as _, Deserialize as _};
            let bytes = param.serialize_as_vec().unwrap();
            let reader = std::io::Cursor::new(&bytes);
            let decoded = ConfigParam::deserialize(reader).unwrap();

            param == decoded
        }

        fn config_param_serialize_readable(param: ConfigParam) -> bool {
            use chain_core::property::Serialize as _;
            let bytes = param.serialize_as_vec().unwrap();
            let mut reader = ReadBuf::from(&bytes);
            let decoded = ConfigParam::read(&mut reader).unwrap();

            param == decoded
        }
    }

    impl Arbitrary for Tag {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            let idx = usize::arbitrary(g) % Tag::iter().count();
            Tag::iter().nth(idx).unwrap()
        }
    }

    impl Arbitrary for Block0Date {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Block0Date(Arbitrary::arbitrary(g))
        }
    }

    impl Arbitrary for Ratio {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            Ratio {
                numerator: Arbitrary::arbitrary(g),
                denominator: NonZeroU64::new(Arbitrary::arbitrary(g))
                    .unwrap_or_else(|| NonZeroU64::new(1).unwrap()),
            }
        }
    }

    impl Arbitrary for RewardParams {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            if bool::arbitrary(g) {
                RewardParams::Linear {
                    constant: Arbitrary::arbitrary(g),
                    ratio: Arbitrary::arbitrary(g),
                    epoch_start: Arbitrary::arbitrary(g),
                    epoch_rate: NonZeroU32::new(20).unwrap(),
                }
            } else {
                RewardParams::Halving {
                    constant: Arbitrary::arbitrary(g),
                    ratio: Arbitrary::arbitrary(g),
                    epoch_start: Arbitrary::arbitrary(g),
                    epoch_rate: NonZeroU32::new(20).unwrap(),
                }
            }
        }
    }

    #[cfg(feature = "evm")]
    impl Arbitrary for EvmConfigParams {
        fn arbitrary<G: Gen>(_g: &mut G) -> Self {
            todo!();
        }
    }

    impl Arbitrary for ConfigParam {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
            match u8::arbitrary(g) % 30 {
                0 => ConfigParam::Block0Date(Arbitrary::arbitrary(g)),
                1 => ConfigParam::Discrimination(Arbitrary::arbitrary(g)),
                2 => ConfigParam::ConsensusVersion(Arbitrary::arbitrary(g)),
                3 => ConfigParam::SlotsPerEpoch(Arbitrary::arbitrary(g)),
                4 => {
                    let mut slot_duration = u8::arbitrary(g);
                    if slot_duration == 0 {
                        slot_duration += 1;
                    }
                    ConfigParam::SlotDuration(slot_duration)
                }
                5 => ConfigParam::ConsensusGenesisPraosActiveSlotsCoeff(Arbitrary::arbitrary(g)),
                6 => ConfigParam::BlockContentMaxSize(Arbitrary::arbitrary(g)),
                7 => ConfigParam::AddBftLeader(Arbitrary::arbitrary(g)),
                8 => ConfigParam::RemoveBftLeader(Arbitrary::arbitrary(g)),
                9 => ConfigParam::LinearFee(Arbitrary::arbitrary(g)),
                10 => ConfigParam::ProposalExpiration(Arbitrary::arbitrary(g)),
                11 => ConfigParam::TreasuryAdd(Arbitrary::arbitrary(g)),
                12 => ConfigParam::RewardPot(Arbitrary::arbitrary(g)),
                13 => ConfigParam::RewardParams(Arbitrary::arbitrary(g)),
                14 => ConfigParam::PerCertificateFees(Arbitrary::arbitrary(g)),
                15 => ConfigParam::FeesInTreasury(Arbitrary::arbitrary(g)),
                16 => ConfigParam::AddCommitteeId(Arbitrary::arbitrary(g)),
                17 => ConfigParam::RemoveCommitteeId(Arbitrary::arbitrary(g)),
                18 => ConfigParam::PerVoteCertificateFees(Arbitrary::arbitrary(g)),
                19 => ConfigParam::RewardPot(Arbitrary::arbitrary(g)),
                20 => ConfigParam::RewardParams(Arbitrary::arbitrary(g)),
                21 => ConfigParam::RewardParams(Arbitrary::arbitrary(g)),
                22 => ConfigParam::FeesInTreasury(Arbitrary::arbitrary(g)),
                23 => ConfigParam::RewardLimitNone,
                24 => ConfigParam::RewardLimitByAbsoluteStake(Arbitrary::arbitrary(g)),
                25 => ConfigParam::PoolRewardParticipationCapping(Arbitrary::arbitrary(g)),
                26 => ConfigParam::AddCommitteeId(Arbitrary::arbitrary(g)),
                27 => ConfigParam::RemoveCommitteeId(Arbitrary::arbitrary(g)),
                28 => ConfigParam::PerCertificateFees(Arbitrary::arbitrary(g)),
                29 => ConfigParam::TransactionMaxExpiryEpochs(Arbitrary::arbitrary(g)),
                #[cfg(feature = "evm")]
                30 => ConfigParam::EvmParams(Arbitrary::arbitrary(g)),
                _ => unreachable!(),
            }
        }
    }
}
