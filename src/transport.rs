//! Bounded Crowsi read-only evidence protocol. No execute/accept/sign command.
use crate::{Error, MAX_EVIDENCE_BYTES};
use crowsi_transport_foundation::{
    Connection, Limits,
    io::{Reader, Writer},
};
use hat_specifications::{HatExecutionEvidence, HatExecutionEvidenceBinding};
use serde::{Deserialize, Serialize};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use zixcel_revision::{CommitStore, RedbBackend, attestation::VerificationMaterial};

const DEADLINE: Duration = Duration::from_secs(4);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LookupRequest {
    pub route_ref: String,
    pub binding: HatExecutionEvidenceBinding,
    pub authority: VerificationMaterial,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "outcome", deny_unknown_fields)]
enum Reply {
    Evidence { evidence: Box<HatExecutionEvidence> },
    Unavailable {},
    Expired {},
    Conflict { references: Vec<String> },
    Rejected {},
    StorageFailure {},
}
fn limits() -> Limits {
    Limits {
        accepted: MAX_EVIDENCE_BYTES,
        buffered: MAX_EVIDENCE_BYTES + 1,
        frame: MAX_EVIDENCE_BYTES + 2,
        pending_bytes: MAX_EVIDENCE_BYTES + 2,
        pending_messages: 1,
    }
}
fn private_parent(socket: &Path) -> Result<std::fs::Metadata, Error> {
    let parent = socket.parent().ok_or(Error::Transport)?;
    let m = std::fs::symlink_metadata(parent).map_err(|_| Error::Transport)?;
    if !socket.is_absolute() || !m.is_dir() || m.permissions().mode() & 0o077 != 0 {
        return Err(Error::Transport);
    }
    Ok(m)
}

/// # Errors
/// Deadline/size/peer failures are transport errors, not an absent external effect.
pub async fn query(
    endpoint: &Path,
    request: &LookupRequest,
) -> Result<HatExecutionEvidence, Error> {
    if !hat_specifications::validate_execution_evidence_binding(&request.binding).valid
        || request.route_ref.len() > 256
        || request.authority.validate().is_err()
    {
        return Err(Error::Invalid);
    }
    let exchange = async {
        let owner = private_parent(endpoint)?;
        let stream = tokio::net::UnixStream::connect(endpoint)
            .await
            .map_err(|_| Error::Transport)?;
        if stream.peer_cred().map_err(|_| Error::Transport)?.uid() != owner.uid() {
            return Err(Error::Transport);
        }
        let connection = Connection::new().map_err(|_| Error::Transport)?;
        connection.open().map_err(|_| Error::Transport)?;
        let (read, write) = stream.into_split();
        let mut reader =
            Reader::new(read, limits(), connection.clone()).map_err(|_| Error::Transport)?;
        let writer = Writer::new(write, limits(), connection.clone(), DEADLINE)
            .map_err(|_| Error::Transport)?;
        writer
            .send(|out| serde_json::to_writer(out, request).map_err(std::io::Error::other))
            .await
            .map_err(|_| Error::Transport)?;
        let frame = reader
            .next_frame()
            .await
            .map_err(|_| Error::Transport)?
            .ok_or(Error::Transport)?;
        connection.close();
        match serde_json::from_slice::<Reply>(&frame.payload).map_err(|_| Error::Proof)? {
            Reply::Evidence { evidence } => {
                crate::verify(&evidence, &request.binding, &request.authority, now()?)?;
                Ok(*evidence)
            }
            Reply::Unavailable {} => Err(Error::Unavailable),
            Reply::Expired {} => Err(Error::Expired),
            Reply::Conflict { references } => {
                if references.len() > 257
                    || references
                        .iter()
                        .any(|r| !zixcel_revision::attestation::valid_digest(r))
                {
                    return Err(Error::Proof);
                }
                Err(Error::Conflict(references))
            }
            Reply::Rejected {} => Err(Error::Proof),
            Reply::StorageFailure {} => Err(Error::Storage(zixcel_revision::Failure::Storage)),
        }
    };
    tokio::time::timeout(DEADLINE, exchange)
        .await
        .map_err(|_| Error::Transport)?
}
fn now() -> Result<u64, Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| Error::Invalid)
}
fn response(database: &Path, route: &str, bytes: &[u8]) -> Reply {
    let result = (|| {
        let request: LookupRequest = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        if request.route_ref != route
            || !hat_specifications::validate_execution_evidence_binding(&request.binding).valid
        {
            return Err(Error::Invalid);
        }
        // Pure reads never create or recover a database. Missing/corrupt storage
        // stays distinguishable from a legitimate absent execution receipt.
        let backend = RedbBackend::open_existing(database)?;
        crate::lookup(
            &CommitStore::new(backend),
            &request.binding,
            &request.authority,
            now()?,
        )
    })();
    match result {
        Ok(evidence) => Reply::Evidence {
            evidence: Box::new(evidence),
        },
        Err(Error::Unavailable) => Reply::Unavailable {},
        Err(Error::Expired) => Reply::Expired {},
        Err(Error::Conflict(references)) => Reply::Conflict { references },
        Err(Error::Storage(_)) => Reply::StorageFailure {},
        Err(_) => Reply::Rejected {},
    }
}
struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.device && m.ino() == self.inode)
        {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
/// One sequential bounded query at a time. No background tasks, worker registry,
/// execution queue or private key. Cancellation drops every socket and its guard.
/// # Errors
/// Invalid private endpoint or I/O failures stop this explicitly launched service.
pub async fn serve_readonly(database: &Path, route: &str, socket: &Path) -> Result<(), Error> {
    let owner = private_parent(socket)?;
    if !route.starts_with("crowsi/") || route.len() > 256 {
        return Err(Error::Invalid);
    }
    // Validate existing storage before advertising readiness; drop the read handle
    // so the independent writer is not prevented from acquiring its own transaction.
    drop(RedbBackend::open_existing(database)?);
    let listener = tokio::net::UnixListener::bind(socket).map_err(|_| Error::Transport)?;
    let metadata = std::fs::symlink_metadata(socket).map_err(|_| Error::Transport)?;
    let _guard = SocketGuard {
        path: socket.to_path_buf(),
        device: metadata.dev(),
        inode: metadata.ino(),
    };
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::Transport)?;
    loop {
        let (stream, _) = listener.accept().await.map_err(|_| Error::Transport)?;
        if stream.peer_cred().map_err(|_| Error::Transport)?.uid() != owner.uid() {
            continue;
        }
        let connection = Connection::new().map_err(|_| Error::Transport)?;
        connection.open().map_err(|_| Error::Transport)?;
        let (read, write) = stream.into_split();
        let mut reader =
            Reader::new(read, limits(), connection.clone()).map_err(|_| Error::Transport)?;
        let writer = Writer::new(write, limits(), connection.clone(), DEADLINE)
            .map_err(|_| Error::Transport)?;
        let exchange = async {
            let frame = reader
                .next_frame()
                .await?
                .ok_or(crowsi_transport_foundation::Outcome::ConnectionClosed)?;
            let reply = response(database, route, &frame.payload);
            writer
                .send(|out| serde_json::to_writer(out, &reply).map_err(std::io::Error::other))
                .await
        };
        // Per-connection malformed input/timeout must not kill the provider service.
        let _ = tokio::time::timeout(DEADLINE, exchange).await;
        connection.close();
    }
}
