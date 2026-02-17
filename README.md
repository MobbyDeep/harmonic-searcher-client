# harmonic-searcher-client

Rust client library for Harmonic's searcher APIs (auth + bundle submission), modeled after the Jito searcher.

## What this crate provides

- Generated gRPC/protobuf clients from Harmonic `searcher-protos`
- Challenge-response authentication (`GenerateAuthChallenge` -> sign -> `GenerateAuthTokens`)
- Automatic access token refresh via `RefreshAccessToken`
- `send_bundle` helper for submitting `bundle.Bundle` payloads
- `bundle_from_raw_transactions` convenience function for `Vec<u8>` transaction bytes

## Install

Add to your project:

```toml
[dependencies]
harmonic-searcher-client = { path = "../harmonic-searcher-client" }
```

## Usage

```rust
use std::sync::Arc;
use harmonic_searcher_client::{bundle_from_raw_transactions, HarmonicSearcherClient, HarmonicSearcherError};

#[tokio::main]
async fn main() -> Result<(), HarmonicSearcherError> {
    // Replace with your 32-byte pubkey and signer implementation.
    let pubkey = [0u8; 32];

    // The signer must return a 64-byte signature over (pubkey || challenge).
    let signer = Arc::new(|payload: &[u8]| -> Result<Vec<u8>, HarmonicSearcherError> {
        // Example only. Integrate your actual ed25519 signing here.
        // Return Err(HarmonicSearcherError::Signer(...)) if signing fails.
        let _ = payload;
        Err(HarmonicSearcherError::Signer(
            "provide real signer implementation".to_string(),
        ))
    });

    let mut client = HarmonicSearcherClient::connect(
        "https://your-harmonic-endpoint.example.com",
        pubkey,
        signer,
    )
    .await?;

    let bundle = bundle_from_raw_transactions(vec![
        vec![1, 2, 3], // tx1 bytes
        vec![4, 5, 6], // tx2 bytes
    ]);

    let uuid = client.send_bundle(bundle).await?;
    println!("bundle uuid: {uuid}");
    Ok(())
}
```

## Protos used

This project vendors proto definitions from:
- https://github.com/harmonic/searcher-protos

Related reference implementation (Jito):
- https://github.com/jito-labs/searcher-examples