use crate::{
    certificate::{CertificateSlice, VotePlanId},
    transaction::{Payload, PayloadAuthData, PayloadData, PayloadSlice},
    vote,
};
use chain_core::property::{Deserialize, ReadError, Serialize, WriteError};
use typed_bytes::{ByteArray, ByteBuilder};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VoteCast {
    vote_plan: VotePlanId,
    proposal_index: u8,
    payload: vote::Payload,
}

impl VoteCast {
    pub fn new(vote_plan: VotePlanId, proposal_index: u8, payload: vote::Payload) -> Self {
        Self {
            vote_plan,
            proposal_index,
            payload,
        }
    }

    pub fn vote_plan(&self) -> &VotePlanId {
        &self.vote_plan
    }

    pub fn proposal_index(&self) -> u8 {
        self.proposal_index
    }

    pub fn payload(&self) -> &vote::Payload {
        &self.payload
    }

    pub(crate) fn into_payload(self) -> vote::Payload {
        self.payload
    }

    pub fn serialize_in(&self, bb: ByteBuilder<Self>) -> ByteBuilder<Self> {
        let bb = bb.bytes(self.vote_plan.as_ref()).u8(self.proposal_index);
        self.payload.serialize_in(bb)
    }

    pub fn serialize(&self) -> ByteArray<Self> {
        self.serialize_in(ByteBuilder::new()).finalize()
    }
}

/* Auth/Payload ************************************************************* */

impl Payload for VoteCast {
    const HAS_DATA: bool = true;
    const HAS_AUTH: bool = false;
    type Auth = ();

    fn payload_data(&self) -> PayloadData<Self> {
        PayloadData(
            self.serialize_in(ByteBuilder::new())
                .finalize_as_vec()
                .into(),
            std::marker::PhantomData,
        )
    }

    fn payload_auth_data(_: &Self::Auth) -> PayloadAuthData<Self> {
        PayloadAuthData(Vec::with_capacity(0).into(), std::marker::PhantomData)
    }

    fn payload_to_certificate_slice(p: PayloadSlice<'_, Self>) -> Option<CertificateSlice<'_>> {
        Some(CertificateSlice::from(p))
    }
}

/* Ser/De ******************************************************************* */

impl Serialize for VoteCast {
    fn serialize<W: std::io::Write>(&self, mut writer: W) -> Result<(), WriteError> {
        writer.write_all(self.serialize().as_slice())?;
        Ok(())
    }
}

impl Deserialize for VoteCast {
    fn deserialize<R: std::io::BufRead>(reader: R) -> Result<Self, ReadError> {
        use chain_core::packer::Codec;

        let mut codec = Codec::new(reader);
        let vote_plan = <[u8; 32]>::deserialize(&mut codec)?.into();
        let proposal_index = codec.get_u8()?;
        let payload = vote::Payload::read(&mut codec)?;

        Ok(Self::new(vote_plan, proposal_index, payload))
    }
}
