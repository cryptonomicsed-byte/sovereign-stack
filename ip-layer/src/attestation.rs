/// Attestation — kind:1902.
///
/// Vouch / challenge / confirm on a Creation Receipt or IP Root.
/// Attestor-agnostic: human, peer agent, or Zàngbétò all use the same shape.
///
/// Schema: schemas/attestation.md in the ip-layer repo.

use crate::nostr::{NostrEvent, NostrSecretKey, sign_event, IpLayerError};

pub const KIND_ATTESTATION: u32 = 1902;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stance {
    /// Attestor affirms the claim is credible.
    Vouch,
    /// Attestor disputes the claim; reason is required.
    Challenge,
    /// Attestor has directly verified a specific fact.
    Confirm,
}

impl Stance {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Vouch     => "vouch",
            Self::Challenge => "challenge",
            Self::Confirm   => "confirm",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AttestationBuilder {
    /// Event id of the subject being attested to (Creation Receipt or IP Root)
    pub subject_event_id:  String,
    /// Pubkey of the subject
    pub subject_pubkey:    String,
    pub stance:            Stance,
    /// Required for Challenge, optional otherwise
    pub reason:            Option<String>,
    /// Optional Zàngbétò stake/reputation weight
    pub weight:            Option<String>,
    pub notes:             String,
}

impl AttestationBuilder {
    pub fn vouch(subject_event_id: impl Into<String>, subject_pubkey: impl Into<String>) -> Self {
        Self {
            subject_event_id: subject_event_id.into(),
            subject_pubkey:   subject_pubkey.into(),
            stance:           Stance::Vouch,
            reason:           None,
            weight:           None,
            notes:            String::new(),
        }
    }

    pub fn challenge(
        subject_event_id: impl Into<String>,
        subject_pubkey:   impl Into<String>,
        reason:           impl Into<String>,
    ) -> Self {
        Self {
            subject_event_id: subject_event_id.into(),
            subject_pubkey:   subject_pubkey.into(),
            stance:           Stance::Challenge,
            reason:           Some(reason.into()),
            weight:           None,
            notes:            String::new(),
        }
    }

    pub fn confirm(subject_event_id: impl Into<String>, subject_pubkey: impl Into<String>) -> Self {
        Self {
            subject_event_id: subject_event_id.into(),
            subject_pubkey:   subject_pubkey.into(),
            stance:           Stance::Confirm,
            reason:           None,
            weight:           None,
            notes:            String::new(),
        }
    }

    pub fn with_weight(mut self, w: impl Into<String>) -> Self {
        self.weight = Some(w.into()); self
    }

    pub fn with_notes(mut self, n: impl Into<String>) -> Self {
        self.notes = n.into(); self
    }

    pub fn sign(self, seckey: &NostrSecretKey, created_at: u64) -> Result<NostrEvent, IpLayerError> {
        if self.stance == Stance::Challenge && self.reason.is_none() {
            return Err(IpLayerError::Validation("challenge stance requires a reason".into()));
        }

        let mut tags = vec![
            vec!["e".into(), self.subject_event_id.clone()],
            vec!["p".into(), self.subject_pubkey.clone()],
            vec!["stance".into(), self.stance.as_str().into()],
            // NIP-32 label — machine-filterable
            vec!["L".into(), "attestation".into()],
            vec!["l".into(), self.stance.as_str().into(), "attestation".into()],
        ];

        if let Some(reason) = &self.reason {
            tags.push(vec!["reason".into(), reason.clone()]);
        }
        if let Some(weight) = &self.weight {
            tags.push(vec!["weight".into(), weight.clone()]);
        }

        let content = serde_json::to_string(&serde_json::json!({ "notes": self.notes }))?;

        sign_event(KIND_ATTESTATION, tags, content, seckey, created_at)
    }
}
