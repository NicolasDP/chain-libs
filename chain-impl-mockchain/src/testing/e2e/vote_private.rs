use crate::testing::VoteTestGen;
use crate::{
    certificate::{TallyDecryptShares, VotePlan},
    fee::LinearFee,
    header::BlockDate,
    testing::{
        ledger::ConfigBuilder,
        scenario::{prepare_scenario, proposal, vote_plan, wallet},
        verifiers::LedgerStateVerifier,
    },
    value::Value,
    vote::{Choice, PayloadType},
};
use chain_vote::{
    committee::MemberSecretKey, MemberCommunicationKey, MemberPublicKey, MemberState, CRS,
};
use rand_chacha::ChaCha20Rng;
use rand_core::{CryptoRng, RngCore, SeedableRng};

const ALICE: &str = "Alice";
const STAKE_POOL: &str = "stake_pool";
const VOTE_PLAN: &str = "fund1";

struct CommitteeMembersManager {
    members: Vec<CommitteeMember>,
}

struct CommitteeMember {
    state: MemberState,
}

impl CommitteeMembersManager {
    pub fn new(rng: &mut (impl RngCore + CryptoRng), threshold: usize, members_no: usize) -> Self {
        let mut public_keys = Vec::new();
        for _ in 0..members_no {
            let private_key = MemberCommunicationKey::new(rng);
            let public_key = private_key.to_public();
            public_keys.push(public_key);
        }

        let crs = CRS::random(rng);

        let mut members = Vec::new();
        for i in 0..members_no {
            let state = MemberState::new(rng, threshold, &crs, &public_keys, i);
            members.push(CommitteeMember { state })
        }

        Self { members }
    }

    pub fn members(&self) -> &[CommitteeMember] {
        &self.members
    }
}

impl CommitteeMember {
    pub fn public_key(&self) -> MemberPublicKey {
        self.state.public_key()
    }

    pub fn secret_key(&self) -> &MemberSecretKey {
        self.state.secret_key()
    }
}

#[test]
pub fn private_vote_cast_action_transfer_to_rewards_all_shares() {
    const MEMBERS_NO: usize = 3;
    const THRESHOLD: usize = 2;

    let favorable = Choice::new(1);

    let mut rng = ChaCha20Rng::from_seed([0u8; 32]);

    let members = CommitteeMembersManager::new(&mut rng, THRESHOLD, MEMBERS_NO);

    let committee_keys = members
        .members()
        .iter()
        .map(|committee_member| committee_member.public_key())
        .collect::<Vec<_>>();

    let (mut ledger, controller) = prepare_scenario()
        .with_config(
            ConfigBuilder::new(0)
                .with_fee(LinearFee::new(1, 1, 1))
                .with_rewards(Value(1000)),
        )
        .with_initials(vec![wallet(ALICE)
            .with(1_000)
            .owns(STAKE_POOL)
            .committee_member()])
        .with_vote_plans(vec![vote_plan(VOTE_PLAN)
            .owner(ALICE)
            .consecutive_epoch_dates()
            .payload_type(PayloadType::Private)
            .committee_keys(committee_keys)
            .with_proposal(
                proposal(VoteTestGen::external_proposal_id())
                    .options(3)
                    .action_transfer_to_rewards(100),
            )])
        .build()
        .unwrap();

    let mut alice = controller.wallet(ALICE).unwrap();
    let vote_plan = controller.vote_plan(VOTE_PLAN).unwrap();
    let proposal = vote_plan.proposal(0);

    controller
        .cast_vote_private(
            &alice,
            &vote_plan,
            &proposal.id(),
            favorable,
            &mut ledger,
            &mut rng,
        )
        .unwrap();
    alice.confirm_transaction();

    ledger.fast_forward_to(BlockDate {
        epoch: 1,
        slot_id: 1,
    });

    controller
        .encrypted_tally(&alice, &vote_plan, &mut ledger)
        .unwrap();
    alice.confirm_transaction();

    let shares = ledger
        .ledger
        .active_vote_plans()
        .iter()
        .find(|c_vote_plan| {
            let vote_plan: VotePlan = vote_plan.clone().into();
            c_vote_plan.id == vote_plan.to_id()
        })
        .unwrap()
        .proposals
        .iter()
        .map(|proposal| {
            proposal
                .tally
                .as_ref()
                .unwrap()
                .private_encrypted()
                .unwrap()
                .0
                .clone()
        })
        .map(|encrypted_tally| {
            members
                .members()
                .iter()
                .map(|member| member.secret_key())
                .map(|secret_key| encrypted_tally.finish(secret_key).1)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let shares = TallyDecryptShares::new(shares);

    controller
        .tally_vote_private(&alice, &vote_plan, shares, &mut ledger)
        .unwrap();

    ledger.fast_forward_to(BlockDate {
        epoch: 1,
        slot_id: 1,
    });

    ledger.apply_protocol_changes().unwrap();

    LedgerStateVerifier::new(ledger.into())
        .info("rewards pot is increased")
        .pots()
        .has_remaining_rewards_equals_to(&Value(1100));
}

#[test]
#[should_panic]
pub fn private_vote_plan_without_keys() {
    let committee_keys = vec![];

    let (_ledger, _controller) = prepare_scenario()
        .with_config(
            ConfigBuilder::new(0)
                .with_fee(LinearFee::new(1, 1, 1))
                .with_rewards(Value(1000)),
        )
        .with_initials(vec![wallet(ALICE)
            .with(1_000)
            .owns(STAKE_POOL)
            .committee_member()])
        .with_vote_plans(vec![vote_plan(VOTE_PLAN)
            .owner(ALICE)
            .consecutive_epoch_dates()
            .payload_type(PayloadType::Private)
            .committee_keys(committee_keys)
            .with_proposal(
                proposal(VoteTestGen::external_proposal_id())
                    .options(3)
                    .action_transfer_to_rewards(100),
            )])
        .build()
        .unwrap();
}
