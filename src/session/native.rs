use super::{
    initial, observation::Observation, require_binding, status, storage, validate_username,
};
use crate::{
    core::{Result, invalid, storage_error},
    network::Settings,
    storage::Store,
};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const HOST: &str = "com.fiverr_agent_mcp.session";
pub const MAX_FRAME: usize = 65536;
pub fn pair(store: &Store, extension_id: &str, username: &str) -> Result<Value> {
    validate_username(username)?;
    if extension_id.len() != 32 || !extension_id.bytes().all(|b| (b'a'..=b'p').contains(&b)) {
        return Err(invalid(
            "Extension ID must contain exactly 32 letters a-p from the installed extension.",
        ));
    }
    let mut state = initial(&Settings::load(store)?, "extension", username);
    state.extension_id = Some(extension_id.into());
    storage::replace(store, state, None)?;
    status(store)
}
pub fn receive(store: &Store, caller: &str, bytes: &[u8]) -> Result<Value> {
    let (state, _) = storage::load(store)?
        .ok_or_else(|| invalid("Pair the browser extension explicitly first."))?;
    let expected_caller = state
        .extension_id
        .as_ref()
        .map(|id| format!("chrome-extension://{id}/"));
    if state.mode != "extension" || expected_caller.as_deref() != Some(caller) {
        return Err(invalid(
            "Native caller does not match the explicitly paired extension.",
        ));
    }
    require_binding(store, &state)?;
    let result: Result<()> = (|| {
        if bytes.len() > MAX_FRAME {
            return Err(invalid("Native message exceeds the 64 KiB limit."));
        }
        let observation: Observation = serde_json::from_slice(bytes)
            .map_err(|_| invalid("Invalid account observation message."))?;
        observation.validate(&state.username)?;
        require_binding(store, &state)
    })();
    storage::record(
        store,
        state.revision,
        if result.is_ok() {
            "account_observed"
        } else {
            "verification_failed"
        },
    )?;
    result?;
    Ok(json!({"ok":true,"status":status(store)?}))
}
pub async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut prefix = [0; 4];
        reader
            .read_exact(&mut prefix)
            .await
            .map_err(|_| invalid("Native message ended before its length prefix."))?;
        let length = u32::from_ne_bytes(prefix) as usize;
        if length == 0 || length > MAX_FRAME {
            return Err(invalid("Native message length must be 1-65536 bytes."));
        }
        let mut bytes = vec![0; length];
        reader
            .read_exact(&mut bytes)
            .await
            .map_err(|_| invalid("Native message ended before its payload."))?;
        Ok(bytes)
    })
    .await
    .map_err(|_| invalid("Native message read deadline exceeded."))?
}
pub async fn write_frame(writer: &mut (impl AsyncWrite + Unpin), value: &Value) -> Result<()> {
    let bytes = serde_json::to_vec(value).map_err(|_| storage_error())?;
    if bytes.len() > MAX_FRAME {
        return Err(invalid("Native reply exceeds the limit."));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        writer
            .write_all(&(bytes.len() as u32).to_ne_bytes())
            .await
            .map_err(|_| storage_error())?;
        writer
            .write_all(&bytes)
            .await
            .map_err(|_| storage_error())?;
        writer.flush().await.map_err(|_| storage_error())
    })
    .await
    .map_err(|_| invalid("Native reply write deadline exceeded."))?
}
pub async fn run(store: &Store, caller: &str) -> Result<Value> {
    let response = match read_frame(&mut tokio::io::stdin()).await {
        Ok(bytes) => receive(store,caller,&bytes),
        Err(e) => Err(e),
    }.unwrap_or_else(|_|json!({"ok":false,"error":"Account observation refused; check pairing, account and Fiverr page."}));
    write_frame(&mut tokio::io::stdout(), &response).await?;
    Ok(Value::Null)
}
pub fn manifest(store: &Store) -> Result<Value> {
    let (state, _) = storage::load(store)?
        .ok_or_else(|| invalid("Pair an extension before generating its host manifest."))?;
    let id = state
        .extension_id
        .filter(|_| state.mode == "extension")
        .ok_or_else(|| invalid("Select extension session mode explicitly first."))?;
    let executable = std::env::current_exe().map_err(|_| storage_error())?;
    store.root.assert_owned(&executable)?;
    // Chrome passes the caller origin to this executable, not an arbitrary command.
    let path = store.root.safe_path("config", &format!("{HOST}.json"))?;
    let value = json!({"name":HOST,"description":"Fiverr account observation bridge","path":dunce::simplified(&executable),"type":"stdio","allowed_origins":[format!("chrome-extension://{id}/")]});
    let bytes = serde_json::to_vec_pretty(&value).map_err(|_| storage_error())?;
    if path.exists() {
        if store.root.read_bounded(&path, 16384)? != bytes {
            return Err(invalid(
                "Native manifest already exists with different settings; preserve and review it before replacement.",
            ));
        }
    } else {
        store.root.write_new(&path, &bytes)?;
    }
    Ok(
        json!({"file":path,"host":HOST,"registered":false,"registration":"Register this exact manifest in your browser's native-messaging host location. No registry or browser setting was changed."}),
    )
}
