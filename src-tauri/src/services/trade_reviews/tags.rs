//! `BehavioralTag` — closed enum the LLM picks from when authoring a
//! trade review. Mirrored 1:1 in `agent/trade_review.py`. A
//! mirror-test (`agent/tests/test_tag_mirror.py`) parses this file
//! and asserts the Python list matches name-for-name.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, Eq, PartialEq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BehavioralTag {
    ChaseOwnExit,
    LateOtmLottery,
    GammaWindowViolation,
    SingleNameConcentration,
    PositionSizingUngraduated,
    PostLossRevenge,
    FlatClose,
    DisciplineOnLoser,
    ScaledInWinner,
    ScaledInLoser,
    ThesisMatchExecuted,
    OffThesisTrade,
}

impl BehavioralTag {
    /// Score weight applied during grade computation.
    pub fn weight(self) -> i32 {
        use BehavioralTag::*;
        match self {
            ChaseOwnExit => -10,
            LateOtmLottery => -10,
            GammaWindowViolation => -5,
            SingleNameConcentration => -5,
            PositionSizingUngraduated => -5,
            PostLossRevenge => -5,
            FlatClose => 5,
            DisciplineOnLoser => 5,
            ScaledInWinner => 3,
            ScaledInLoser => -7,
            ThesisMatchExecuted => 5,
            OffThesisTrade => -3,
        }
    }

    /// All values in declaration order. Used by the mirror-test and the
    /// LLM's tool schema.
    pub const ALL: [BehavioralTag; 12] = [
        BehavioralTag::ChaseOwnExit,
        BehavioralTag::LateOtmLottery,
        BehavioralTag::GammaWindowViolation,
        BehavioralTag::SingleNameConcentration,
        BehavioralTag::PositionSizingUngraduated,
        BehavioralTag::PostLossRevenge,
        BehavioralTag::FlatClose,
        BehavioralTag::DisciplineOnLoser,
        BehavioralTag::ScaledInWinner,
        BehavioralTag::ScaledInLoser,
        BehavioralTag::ThesisMatchExecuted,
        BehavioralTag::OffThesisTrade,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_close_is_positive() {
        assert!(BehavioralTag::FlatClose.weight() > 0);
    }

    #[test]
    fn chase_own_exit_is_strongly_negative() {
        assert!(BehavioralTag::ChaseOwnExit.weight() <= -10);
    }

    #[test]
    fn all_has_no_duplicates() {
        let mut sorted = BehavioralTag::ALL
            .iter()
            .map(|t| format!("{:?}", t))
            .collect::<Vec<_>>();
        sorted.sort();
        let mut dedup = sorted.clone();
        dedup.dedup();
        assert_eq!(
            dedup.len(),
            sorted.len(),
            "ALL has duplicates: {:?}",
            sorted
        );
    }

    #[test]
    fn all_serialises_to_snake_case() {
        let json = serde_json::to_string(&BehavioralTag::ChaseOwnExit).expect("serde");
        assert_eq!(json, "\"chase_own_exit\"");
        let json = serde_json::to_string(&BehavioralTag::LateOtmLottery).expect("serde");
        assert_eq!(json, "\"late_otm_lottery\"");
    }
}
