use hat_execution_evidence::{Authority, Error, lookup, verify};
use hat_specifications::*;
use zixcel_revision::attestation::{VerificationMaterial, content_digest};
use zixcel_revision::{CommitStore, MemoryBackend, RedbBackend};

const SECRET: [u8; 32] = [71; 32];

#[cfg(all(unix, feature = "transport"))]
#[tokio::test(flavor = "current_thread")]
async fn external_keyless_lookup_is_bounded_readonly_and_never_executes() {
    use hat_execution_evidence::transport::{LookupRequest, query};
    use std::{
        os::unix::fs::PermissionsExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    struct Owned(std::process::Child);
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let database = dir.path().join("evidence.redb");
    let socket = dir.path().join("lookup.sock");
    let authority =
        Authority::new(RedbBackend::create(&database).unwrap(), trust(), &SECRET).unwrap();
    let mut statement = statement();
    statement.produced_at_epoch_s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    statement.expires_at_epoch_s = None;
    let permit = authority.accept(statement.binding.clone()).unwrap();
    let proof = authority.record_result(&permit, statement.clone()).unwrap();
    drop(authority);
    let before = std::fs::read(&database).unwrap();
    let mut process = Owned(
        Command::new(env!("CARGO_BIN_EXE_hat-evidence-lookup"))
            .arg(&database)
            .arg("crowsi/evidence/reference")
            .arg(&socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    while !socket.exists() {
        assert!(Instant::now() < deadline && process.0.try_wait().unwrap().is_none());
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let mut request = LookupRequest {
        route_ref: "crowsi/evidence/reference".into(),
        binding: statement.binding,
        authority: trust(),
    };
    assert_eq!(query(&socket, &request).await.unwrap(), proof);
    request.route_ref = "crowsi/evidence/wrong".into();
    assert!(matches!(query(&socket, &request).await, Err(Error::Proof)));
    request.route_ref = "crowsi/evidence/reference".into();
    request.binding.worker_generation = "replacement-g2".into();
    assert!(matches!(query(&socket, &request).await, Err(Error::Proof)));
    request.binding = proof.statement.binding.clone();
    assert_eq!(query(&socket, &request).await.unwrap(), proof);
    assert!(matches!(
        query(&dir.path().join("absent.sock"), &request).await,
        Err(Error::Transport)
    ));
    assert_eq!(std::fs::read(&database).unwrap(), before);
    drop(process);
}
fn trust() -> VerificationMaterial {
    VerificationMaterial {
        authority: "provider/evidence".into(),
        epoch: 3,
        public_key: ed25519_dalek::SigningKey::from_bytes(&SECRET)
            .verifying_key()
            .to_bytes(),
    }
}
fn reference(kind: &str) -> ActionReference {
    ActionReference {
        owner_id: "provider".into(),
        reference: kind.into(),
        schema_id: format!("hathq://provider/{kind}/v1"),
        digest_sha256: content_digest(kind.as_bytes()),
    }
}
fn statement() -> HatExecutionEvidenceStatement {
    let invocation = HatInvocation {
        schema: INVOCATION_SCHEMA.into(),
        invocation_id: "original-invocation".into(),
        binding: HatBinding {
            schema: BINDING_SCHEMA.into(),
            package_id: "hat/reference-provider".into(),
            package_digest_sha256: content_digest(b"package"),
            catalog_digest_sha256: content_digest(b"catalog"),
            fitting_digest_sha256: content_digest(b"fitting"),
            subject_ref: "subject".into(),
            scope_ref: "scope".into(),
        },
        context_partition: ContextPartition {
            context_partition_id: "partition".into(),
            revision: 1,
            policy_digest_sha256: content_digest(b"policy"),
        },
        operation_id: "hathq://provider/action/review/v1".into(),
        expected_projection_revision: 0,
        idempotency_key: "original-operation".into(),
        input: reference("input"),
        effective_grant: reference("grant"),
        placement: reference("placement"),
    };
    HatExecutionEvidenceStatement {
        binding: HatExecutionEvidenceBinding {
            execution_ref: reference("execution"),
            invocation: invocation.clone(),
            provider_ref: reference("provider"),
            provider_generation: "provider-g1".into(),
            worker_ref: reference("worker"),
            worker_generation: "worker-g1".into(),
            capability_ref: reference("capability"),
        },
        result: HatActionResult {
            schema: ACTION_RESULT_SCHEMA.into(),
            invocation_id: invocation.invocation_id,
            operation_id: invocation.operation_id,
            state_revision: 3,
            projection_revision: 1,
            outcome: HatInvocationOutcome::Completed,
            output: Some(reference("output")),
            reason_id: None,
            evidence_refs: vec![],
        },
        provider_receipt: None,
        failure: None,
        produced_at_epoch_s: 100,
        expires_at_epoch_s: Some(200),
    }
}

#[test]
fn exact_generation_authority_claim_tampering_replay_expiry_and_conflict() {
    let authority = Authority::new(MemoryBackend::default(), trust(), &SECRET).unwrap();
    let statement = statement();
    assert!(validate_execution_evidence_statement(&statement).valid);
    let permit = authority.accept(statement.binding.clone()).unwrap();
    assert!(matches!(
        authority.lookup(&statement.binding, 110),
        Err(Error::Unavailable)
    ));
    let proof = authority.record_result(&permit, statement.clone()).unwrap();
    assert_eq!(
        authority.record_result(&permit, statement.clone()).unwrap(),
        proof
    );
    assert_eq!(authority.lookup(&statement.binding, 110).unwrap(), proof);
    assert!(matches!(
        authority.accept(statement.binding.clone()),
        Err(Error::AlreadyAccepted)
    ));
    assert!(matches!(
        authority.lookup(&statement.binding, 200),
        Err(Error::Expired)
    ));
    for field in 0..13 {
        let mut wrong = proof.clone();
        match field {
            0 => wrong.statement.binding.worker_generation = "worker-g2".into(),
            1 => wrong.statement.binding.invocation.invocation_id = "other-invocation".into(),
            2 => wrong.statement.binding.execution_ref = reference("other-execution"),
            3 => {
                wrong.statement.binding.invocation.input.digest_sha256 =
                    content_digest(b"other-input");
            }
            4 => wrong.statement.result.output = Some(reference("other-output")),
            5 => wrong.statement.binding.provider_ref = reference("other-provider"),
            6 => wrong.statement.binding.provider_generation = "provider-g2".into(),
            7 => wrong.attestation.statement.authority = "attacker".into(),
            8 => wrong.attestation.statement.authority_epoch += 1,
            9 => wrong.attestation.signature.clear(),
            10 => wrong.attestation.signature[0] ^= 1,
            11 => wrong.statement.expires_at_epoch_s = None,
            _ => wrong.statement.binding.capability_ref = reference("other-action"),
        }
        assert!(
            verify(&wrong, &statement.binding, &trust(), 110).is_err(),
            "field {field}"
        );
    }
    let replacement = Authority::new(MemoryBackend::default(), trust(), &SECRET).unwrap();
    assert!(matches!(
        replacement.record_result(&permit, statement.clone()),
        Err(Error::Invalid)
    ));
    let mut conflict = statement.clone();
    conflict.result.output = Some(reference("conflicting-output"));
    assert!(matches!(
        authority.record_result(&permit, conflict),
        Err(Error::Conflict(_))
    ));
    let Err(Error::Conflict(refs)) = authority.lookup(&statement.binding, 110) else {
        panic!("no latest wins")
    };
    assert_eq!(refs.len(), 2);
    assert!(refs.contains(&proof.attestation.reference().unwrap()));
    // A later exact replay cannot erase the preserved conflict.
    assert_eq!(
        authority.record_result(&permit, statement.clone()).unwrap(),
        proof
    );
    assert!(matches!(
        authority.lookup(&statement.binding, 110),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn durable_original_proof_survives_authority_restart_without_any_write_key() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("evidence.redb");
    let statement = statement();
    let proof = {
        let authority =
            Authority::new(RedbBackend::create(&path).unwrap(), trust(), &SECRET).unwrap();
        let permit = authority.accept(statement.binding.clone()).unwrap();
        authority.record_result(&permit, statement.clone()).unwrap()
    };
    let before = std::fs::read(&path).unwrap();
    let read_only = CommitStore::new(RedbBackend::open_existing(&path).unwrap());
    assert_eq!(
        lookup(&read_only, &statement.binding, &trust(), 110).unwrap(),
        proof
    );
    assert_eq!(std::fs::read(&path).unwrap(), before);
    drop(read_only);
    let replacement =
        Authority::new(RedbBackend::open_existing(&path).unwrap(), trust(), &SECRET).unwrap();
    assert!(matches!(
        replacement.accept(statement.binding.clone()),
        Err(Error::AlreadyAccepted)
    ));
    assert_eq!(replacement.lookup(&statement.binding, 110).unwrap(), proof);
    let arbitrary = root.path().join("not-evidence.redb");
    std::fs::write(&arbitrary, serde_json::to_vec(&proof).unwrap()).unwrap();
    assert!(RedbBackend::open_existing(&arbitrary).is_err());
    let absent = root.path().join("absent.redb");
    assert!(RedbBackend::open_existing(&absent).is_err());
    assert!(!absent.exists());
}
