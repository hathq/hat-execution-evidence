//! The provider owns historical statements; workers are disposable producers.
use ed25519_dalek::SigningKey;
use hat_specifications::{
    HatExecutionEvidence, HatExecutionEvidenceBinding, HatExecutionEvidenceStatement,
    validate_execution_evidence_statement,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use zixcel_revision::attestation::{
    Attestation, Statement, VerificationMaterial, content_digest, valid_digest,
};
use zixcel_revision::{
    Backend, CommitIntent, CommitOutcome, CommitReceipt, CommitStore, RevisionRef,
};

pub const MAX_EVIDENCE_BYTES: usize = 65_536;

#[cfg(all(unix, feature = "transport"))]
pub mod transport;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("historical evidence query transport failed")]
    Transport,
    #[error("execution evidence is invalid")]
    Invalid,
    #[error("execution evidence proof rejected")]
    Proof,
    #[error("original execution was already admitted; lookup only")]
    AlreadyAccepted,
    #[error("historical execution evidence is unavailable")]
    Unavailable,
    #[error("historical execution evidence expired")]
    Expired,
    #[error("conflicting execution evidence: {0:?}")]
    Conflict(Vec<String>),
    #[error("evidence storage: {0}")]
    Storage(#[from] zixcel_revision::Failure),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Record {
    Accepted {
        binding: Box<HatExecutionEvidenceBinding>,
        authority: VerificationMaterial,
    },
    Result {
        evidence: Box<HatExecutionEvidence>,
    },
}

/// Non-serializable, authority-instance-bound write permission. Only the
/// protected provider owner holds it; public history cannot reconstruct it.
pub struct ExecutionPermit {
    issuer: Arc<()>,
    binding: HatExecutionEvidenceBinding,
    accepted: CommitReceipt,
}

pub struct Authority<B> {
    store: CommitStore<B>,
    material: VerificationMaterial,
    key: SigningKey,
    incarnation: Arc<()>,
}

impl<B: Backend> Authority<B> {
    /// The caller owns key custody and authorizes use of this protected API.
    /// # Errors
    /// The private key must match the independently declared public material.
    pub fn new(
        backend: B,
        material: VerificationMaterial,
        secret: &[u8; 32],
    ) -> Result<Self, Error> {
        material.validate().map_err(|_| Error::Proof)?;
        let key = SigningKey::from_bytes(secret);
        if key.verifying_key().to_bytes() != material.public_key {
            return Err(Error::Proof);
        }
        Ok(Self {
            store: CommitStore::new(backend),
            material,
            key,
            incarnation: Arc::new(()),
        })
    }

    /// Called only after original execution/worker authorization by the provider.
    /// Exact replay never grants a replacement worker a historical write permit.
    /// # Errors
    /// Invalid or previously admitted identities reject; no provider effect occurs.
    pub fn accept(&self, binding: HatExecutionEvidenceBinding) -> Result<ExecutionPermit, Error> {
        if !hat_specifications::validate_execution_evidence_binding(&binding).valid {
            return Err(Error::Invalid);
        }
        let domain = stream(&binding)?;
        let payload = encode(&Record::Accepted {
            binding: Box::new(binding.clone()),
            authority: self.material.clone(),
        })?;
        let prepared = CommitIntent {
            domain,
            operation_id: "accepted".into(),
            expected_revision: RevisionRef::default(),
            parents: vec![],
            payload,
        }
        .prepare()
        .map_err(|_| Error::Invalid)?;
        match self.store.commit(&prepared) {
            CommitOutcome::Committed(accepted) => Ok(ExecutionPermit {
                issuer: Arc::clone(&self.incarnation),
                binding,
                accepted,
            }),
            CommitOutcome::NoChange(_) | CommitOutcome::Conflict(_) => Err(Error::AlreadyAccepted),
            CommitOutcome::Failure(e) => Err(e.into()),
            CommitOutcome::Rejected(_) => Err(Error::Invalid),
        }
    }

    /// Known result → durable attestation → caller may notify Hatter. Never performs
    /// an external effect. A conflicting proposal remains immutable and inspectable.
    /// # Errors
    /// Rejects another authority's permit or any changed original execution binding.
    pub fn record_result(
        &self,
        permit: &ExecutionPermit,
        statement: HatExecutionEvidenceStatement,
    ) -> Result<HatExecutionEvidence, Error> {
        if !Arc::ptr_eq(&self.incarnation, &permit.issuer)
            || statement.binding != permit.binding
            || !validate_execution_evidence_statement(&statement).valid
        {
            return Err(Error::Invalid);
        }
        let attestation = Attestation::sign(
            proof_statement(&statement, &self.material)?,
            &self.key.to_bytes(),
        )
        .map_err(|_| Error::Proof)?;
        let evidence = HatExecutionEvidence {
            statement,
            attestation,
        };
        let reference = evidence.attestation.reference().map_err(|_| Error::Proof)?;
        let prepared = CommitIntent {
            domain: permit.accepted.domain.clone(),
            operation_id: reference.clone(),
            expected_revision: permit.accepted.committed_revision.clone(),
            parents: vec![permit.accepted.commit_ref.clone()],
            payload: encode(&Record::Result {
                evidence: Box::new(evidence.clone()),
            })?,
        }
        .prepare()
        .map_err(|_| Error::Invalid)?;
        self.store.stage(&prepared)?;
        match self.store.commit(&prepared) {
            CommitOutcome::Committed(_) | CommitOutcome::NoChange(_) => Ok(evidence),
            CommitOutcome::Conflict(_) => Err(Error::Conflict(vec![reference])),
            CommitOutcome::Failure(e) => Err(e.into()),
            CommitOutcome::Rejected(_) => Err(Error::Invalid),
        }
    }

    /// # Errors
    /// Same read-only historical inspection as the standalone lookup function.
    pub fn lookup(
        &self,
        binding: &HatExecutionEvidenceBinding,
        now: u64,
    ) -> Result<HatExecutionEvidence, Error> {
        lookup(&self.store, binding, &self.material, now)
    }
}

/// Side-effect-free lookup. This API requires no private key, current worker or
/// execution permission. Trust material comes from ORIGINAL accepted placement.
/// # Errors
/// Missing, expired, forged or divergent evidence stays distinct; never retry work.
pub fn lookup<B: Backend>(
    store: &CommitStore<B>,
    binding: &HatExecutionEvidenceBinding,
    trusted: &VerificationMaterial,
    now: u64,
) -> Result<HatExecutionEvidence, Error> {
    let domain = stream(binding)?;
    let accepted = store
        .receipt(&domain, "accepted")?
        .ok_or(Error::Unavailable)?;
    let proposal = store
        .committed(&accepted.commit_ref)?
        .ok_or(Error::Unavailable)?;
    if decode(proposal.payload())?
        != (Record::Accepted {
            binding: Box::new(binding.clone()),
            authority: trusted.clone(),
        })
    {
        return Err(Error::Proof);
    }
    let mut proofs = std::collections::BTreeMap::new();
    for receipt in store.transitions(&domain, accepted.committed_revision.sequence, 256)? {
        let object = store
            .committed(&receipt.commit_ref)?
            .ok_or(Error::Unavailable)?;
        collect(object.payload(), binding, trusted, &mut proofs)?;
    }
    // Prepared is never a canonical success; signed divergent objects must still
    // be seen to prevent latest-wins. They cannot be ignored merely for losing CAS.
    let committed_count = proofs.len();
    for reference in store.prepared_references(&domain)? {
        let object = store.prepared(&reference)?.ok_or(Error::Unavailable)?;
        collect(object.payload(), binding, trusted, &mut proofs)?;
    }
    if proofs.len() > 1 {
        return Err(Error::Conflict(proofs.into_keys().collect()));
    }
    if committed_count == 0 {
        return Err(Error::Unavailable);
    }
    let evidence = proofs.into_values().next().ok_or(Error::Unavailable)?;
    verify(&evidence, binding, trusted, now)?;
    Ok(evidence)
}

/// Verify a bounded result envelope against independently captured original refs.
/// # Errors
/// Exact mismatch, invalid signature or signed expiry is a refusal, not absence.
pub fn verify(
    evidence: &HatExecutionEvidence,
    expected: &HatExecutionEvidenceBinding,
    trusted: &VerificationMaterial,
    now: u64,
) -> Result<(), Error> {
    if evidence.statement.binding != *expected
        || !validate_execution_evidence_statement(&evidence.statement).valid
        || evidence.attestation.statement != proof_statement(&evidence.statement, trusted)?
    {
        return Err(Error::Proof);
    }
    evidence
        .attestation
        .verify(trusted)
        .map_err(|_| Error::Proof)?;
    if now < evidence.statement.produced_at_epoch_s {
        return Err(Error::Invalid);
    }
    if evidence
        .statement
        .expires_at_epoch_s
        .is_some_and(|t| now >= t)
    {
        return Err(Error::Expired);
    }
    Ok(())
}

fn collect(
    bytes: &[u8],
    binding: &HatExecutionEvidenceBinding,
    trusted: &VerificationMaterial,
    proofs: &mut std::collections::BTreeMap<String, HatExecutionEvidence>,
) -> Result<(), Error> {
    let Record::Result { evidence } = decode(bytes)? else {
        return Err(Error::Invalid);
    };
    verify(
        &evidence,
        binding,
        trusted,
        evidence.statement.produced_at_epoch_s,
    )?;
    proofs.insert(
        evidence.attestation.reference().map_err(|_| Error::Proof)?,
        *evidence,
    );
    Ok(())
}
fn proof_statement(
    statement: &HatExecutionEvidenceStatement,
    trusted: &VerificationMaterial,
) -> Result<Statement, Error> {
    Ok(Statement {
        subject_ref: statement.binding.execution_ref.digest_sha256.clone(),
        authority: trusted.authority.clone(),
        authority_epoch: trusted.epoch,
        claim_digest: content_digest(&encode(statement)?),
        provenance_refs: vec![
            hat_specifications::invocation_digest(&statement.binding.invocation)
                .map_err(|_| Error::Invalid)?,
            statement.binding.invocation.input.digest_sha256.clone(),
        ],
    })
}
fn stream(binding: &HatExecutionEvidenceBinding) -> Result<String, Error> {
    if !valid_digest(&binding.execution_ref.digest_sha256) {
        return Err(Error::Invalid);
    }
    Ok(format!("execution/{}", binding.execution_ref.digest_sha256))
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid)?;
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(Error::Invalid);
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<Record, Error> {
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err(Error::Invalid);
    }
    serde_json::from_slice(bytes).map_err(|_| Error::Invalid)
}
