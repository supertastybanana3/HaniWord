use std::sync::Arc;
use azookey_server::{ConnectionInfo, ConnectionState};
use tonic::{Request, Status};

pub const MAX_INPUT_CHARS: usize = 1024;
pub const MAX_TEXT_BYTES: usize = 8192;
pub const MAX_CONTEXT_BYTES: usize = 16384;
pub const MAX_MESSAGE_BYTES: usize = 65536;
pub const MAX_JOBS: usize = 64;

pub fn peer<T>(request: &Request<T>, persist: bool) -> Result<Arc<ConnectionState>, Status> {
    let info = request.extensions().get::<ConnectionInfo>()
        .ok_or_else(|| Status::unauthenticated("Named-pipe identity required"))?;
    let peer = info.0.peer.get().ok_or_else(|| Status::unauthenticated("Pipe client not authenticated"))?;
    if persist && !peer.may_persist {
        return Err(Status::permission_denied("Restricted clients cannot change persistent learning or configuration"));
    }
    Ok(info.0.clone())
}

pub fn text(value: &str, bytes: usize) -> Result<(), Status> {
    if value.contains('\0') { return Err(Status::invalid_argument("NUL is not allowed")); }
    if value.len() > bytes { return Err(Status::resource_exhausted("Text limit exceeded")); }
    Ok(())
}

pub fn input(value: &str) -> Result<(), Status> {
    text(value, MAX_TEXT_BYTES)?;
    if value.chars().count() > MAX_INPUT_CHARS { return Err(Status::resource_exhausted("Composition limit exceeded")); }
    Ok(())
}

pub fn offset(value: i32, nonnegative: bool) -> Result<(), Status> {
    if value < -(MAX_INPUT_CHARS as i32) || value > MAX_INPUT_CHARS as i32 || (nonnegative && value < 0) {
        return Err(Status::invalid_argument("Cursor offset out of range"));
    }
    Ok(())
}

/// 2026-10-29: 学習した語を忘れる (Ctrl+Delete) ときの読みと表記
pub fn forgetting(request: &shared::proto::ForgetRequest) -> Result<(), Status> {
    for value in [&request.reading, &request.text] {
        input(value)?;
        if value.is_empty() || value.contains('\u{1f}') { return Err(Status::invalid_argument("Invalid forget target")); }
    }
    Ok(())
}

pub fn learning(request: &shared::proto::LearnRequest) -> Result<(), Status> {
    if request.readings.len() != request.texts.len() || request.texts.is_empty() || request.texts.len() > 128 {
        return Err(Status::invalid_argument("Invalid learning segment count"));
    }
    text(&request.context, MAX_CONTEXT_BYTES)?;
    let mut total = 0;
    for value in request.readings.iter().chain(&request.texts) {
        input(value)?;
        if value.is_empty() || value.contains('\u{1f}') { return Err(Status::invalid_argument("Invalid learning segment")); }
        total += value.len();
    }
    if total > MAX_MESSAGE_BYTES / 2 { return Err(Status::resource_exhausted("Learning request too large")); }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;
    #[test]
    fn rejects_nul_and_oversized_unicode_before_ffi() {
        assert_eq!(input("abc\0def").unwrap_err().code(), Code::InvalidArgument);
        assert!(input(&"あ".repeat(MAX_INPUT_CHARS)).is_ok());
        assert_eq!(input(&"あ".repeat(MAX_INPUT_CHARS + 1)).unwrap_err().code(), Code::ResourceExhausted);
        assert_eq!(text(&"a".repeat(MAX_CONTEXT_BYTES + 1), MAX_CONTEXT_BYTES).unwrap_err().code(), Code::ResourceExhausted);
    }
    #[test]
    fn offsets_do_not_wrap_to_i8_or_accept_negative_shrink() {
        assert!(offset(128, false).is_ok());
        assert!(offset(-128, false).is_ok());
        assert!(offset(i32::MAX, false).is_err());
        assert!(offset(i32::MIN, false).is_err());
        assert!(offset(-1, true).is_err());
    }
    #[test]
    fn validates_learning_delimiters_and_matching_counts() {
        let mut request = shared::proto::LearnRequest { readings: vec!["つじ".into()], texts: vec!["辻\u{e0100}".into()], context: String::new() };
        assert!(learning(&request).is_ok());
        request.texts[0] = "x\u{1f}y".into();
        assert!(learning(&request).is_err());
        request.texts.clear();
        assert!(learning(&request).is_err());
    }
    #[test]
    fn rpc_metadata_cannot_supply_transport_identity() {
        let mut request = Request::new(());
        request.metadata_mut().insert("connection-id", "1".parse().unwrap());
        assert!(matches!(peer(&request, false), Err(status) if status.code() == Code::Unauthenticated));
    }
    #[test]
    fn bounded_queue_rejects_without_blocking() {
        let (tx, _rx) = std::sync::mpsc::sync_channel(MAX_JOBS);
        for _ in 0..MAX_JOBS { tx.try_send(()).unwrap(); }
        assert!(matches!(tx.try_send(()), Err(std::sync::mpsc::TrySendError::Full(_))));
    }
}
