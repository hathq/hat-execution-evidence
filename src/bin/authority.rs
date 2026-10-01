//! Protected provider-side result producer. The launcher authorizes one exact
//! execution via a private plan and owns stdin. The disposable worker receives
//! neither the plan nor signing key. Public lookup is a different keyless binary.
use crowsi_transport_foundation::{
    Connection, Limits,
    io::{Reader, Writer},
};
use hat_execution_evidence::{Authority, MAX_EVIDENCE_BYTES};
use hat_specifications::{HatExecutionEvidenceBinding, HatExecutionEvidenceStatement};
use serde::Deserialize;
use std::{io::Read, path::Path, time::Duration};
use zeroize::{Zeroize, Zeroizing};
use zixcel_revision::{RedbBackend, attestation::VerificationMaterial};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    authority: VerificationMaterial,
    signing_key: [u8; 32],
    binding: HatExecutionEvidenceBinding,
}
impl Drop for Plan {
    fn drop(&mut self) {
        self.signing_key.zeroize();
    }
}

#[cfg(unix)]
async fn run(mode: &str, plan: &Path, database: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::symlink_metadata(plan)?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > u64::try_from(MAX_EVIDENCE_BYTES)?
    {
        return Err("private owner plan required".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(plan)?
        .take(u64::try_from(MAX_EVIDENCE_BYTES)? + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_EVIDENCE_BYTES {
        return Err("owner plan exceeds limit".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes)?;
    plan.authority.validate()?;
    if !hat_specifications::validate_execution_evidence_binding(&plan.binding).valid
        || ed25519_dalek::SigningKey::from_bytes(&plan.signing_key)
            .verifying_key()
            .to_bytes()
            != plan.authority.public_key
    {
        return Err("owner plan rejected".into());
    }
    let backend = match mode {
        "initialize" => RedbBackend::create(database)?,
        "existing" => RedbBackend::recover_existing(database)?,
        _ => return Err("explicit initialize or existing mode required".into()),
    };
    let authority = Authority::new(backend, plan.authority.clone(), &plan.signing_key)?;
    let permit = authority.accept(plan.binding.clone())?;
    let connection = Connection::new()?;
    connection.open()?;
    let limits = Limits {
        accepted: MAX_EVIDENCE_BYTES,
        buffered: MAX_EVIDENCE_BYTES + 1,
        frame: MAX_EVIDENCE_BYTES + 2,
        pending_bytes: MAX_EVIDENCE_BYTES + 2,
        pending_messages: 1,
    };
    let mut reader = Reader::new(tokio::io::stdin(), limits, connection.clone())?;
    let writer = Writer::new(
        tokio::io::stdout(),
        limits,
        connection.clone(),
        Duration::from_secs(4),
    )?;
    writer
        .send(|out| {
            serde_json::to_writer(
                out,
                &serde_json::json!({"accepted":plan.binding.execution_ref}),
            )
            .map_err(std::io::Error::other)
        })
        .await?;
    // One exact result only. Deadline is the protected reference-provider bound,
    // not an implicit worker timeout/retry policy.
    let frame = tokio::time::timeout(Duration::from_secs(30), reader.next_frame())
        .await??
        .ok_or("result unavailable")?;
    let statement: HatExecutionEvidenceStatement = serde_json::from_slice(&frame.payload)?;
    let evidence = authority.record_result(&permit, statement)?;
    drop(authority); // durable writer released before notification/query access
    writer.send(|out| serde_json::to_writer(out, &serde_json::json!({"attestation":evidence.attestation.reference().map_err(std::io::Error::other)?})).map_err(std::io::Error::other)).await?;
    connection.close();
    Ok(())
}
#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [mode, plan, database] => {
            run(
                mode.to_str().unwrap_or(""),
                Path::new(plan),
                Path::new(database),
            )
            .await
        }
        _ => Err("usage: hat-evidence-authority initialize|existing PRIVATE_PLAN DATABASE".into()),
    };
    if result.is_err() {
        eprintln!("execution evidence authority failed");
        std::process::exit(1);
    }
}
#[cfg(not(unix))]
fn main() {
    eprintln!("protected local evidence authority unavailable");
    std::process::exit(2);
}
