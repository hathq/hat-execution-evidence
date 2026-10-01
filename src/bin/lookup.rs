//! Explicit provider-owned, keyless lookup process; Hatter never spawns it.
#[cfg(unix)]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [database, route, socket] = args.as_slice() else {
        eprintln!("usage: hat-evidence-lookup DATABASE ROUTE SOCKET");
        std::process::exit(2);
    };
    let Some(route) = route.to_str() else {
        std::process::exit(2)
    };
    let service = hat_execution_evidence::transport::serve_readonly(
        std::path::Path::new(database),
        route,
        std::path::Path::new(socket),
    );
    let result = tokio::select! {
        result = service => result,
        signal = tokio::signal::ctrl_c() => signal.map_err(|_| hat_execution_evidence::Error::Transport),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(unix))]
fn main() {
    eprintln!("local Unix evidence transport unavailable");
    std::process::exit(2);
}
