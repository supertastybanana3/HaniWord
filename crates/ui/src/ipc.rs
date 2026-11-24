use shared::proto::{
    window_service_server::WindowService as WindowServiceProto, EmptyResponse, SetCandidateRequest,
    SetInputModeRequest, SetPositionRequest, SetSelectionRequest,
};
use crate::owner::{Gate, Kind};
use std::sync::Arc;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};

#[derive(Debug, Clone)]
pub struct WindowController {
    sender: mpsc::Sender<WindowAction>,
}

impl WindowController {
    pub fn new(sender: mpsc::Sender<WindowAction>) -> Self {
        Self { sender }
    }
}

// ウィンドウ操作コマンド
#[derive(Debug, serde::Serialize)]
pub enum WindowAction {
    Show,
    Hide,
    SetPosition {
        top: i32,
        left: i32,
        bottom: i32,
        right: i32,
    },
    SetSelection {
        index: i32,
    },
    SetCandidate {
        candidates: Vec<String>,
        /// 2026-11-08 (仕事 52): 候補の窓の下の欄に添える文言 (空なら添えない)
        note: String,
    },
    /// 入力モード (あ・A) と、AI を休んでいるか (2026-11-08 仕事 52。札の右下の点)
    SetInputMode(String, bool),
}

pub struct WindowService {
    pub controller: WindowController,
    /// 2026-11-23 (点検 Astra 02): 表示を変える要求は、今の入力先の処理からの接続だけ (owner.rs)
    pub gate: Arc<Gate>,
}

/// 試験だけで使う相手 (ui の単体の試験の中だけ。本番の組み立てには入らない)
#[cfg(test)]
#[derive(Clone, Copy)]
pub struct TestPeer(pub u64, pub u32);

/// 要求の相手 (接続の番号と処理の番号)。パイプの受け口が入れたものだけを使う
fn peer<T>(request: &Request<T>) -> crate::owner::Peer {
    #[cfg(test)]
    if let Some(test) = request.extensions().get::<TestPeer>() { return Some((test.0, test.1)); }
    let info = request.extensions().get::<azookey_server::ConnectionInfo>()?;
    Some((info.0.id, *info.0.pid.get()?))
}

impl WindowService {
    /// 通さない要求は、何もせずに Ok を返す (前面が移る間際の正しい要求で IME の側を崩さないため。数は Gate が数える)
    fn allowed<T>(&self, request: &Request<T>, kind: Kind) -> bool {
        self.gate.allow(peer(request), kind)
    }
}

#[tonic::async_trait]
impl WindowServiceProto for WindowService {
    async fn show_window(
        &self,
        request: Request<EmptyResponse>,
    ) -> Result<Response<EmptyResponse>, Status> {
        if !self.allowed(&request, Kind::Show) { return Ok(Response::new(EmptyResponse {})); }
        self.controller
            .sender
            .send(WindowAction::Show)
            .await
            .map_err(|_| Status::unavailable("Candidate UI stopped"))?;
        Ok(Response::new(EmptyResponse {}))
    }

    async fn hide_window(
        &self,
        request: Request<EmptyResponse>,
    ) -> Result<Response<EmptyResponse>, Status> {
        if !self.allowed(&request, Kind::Hide) { return Ok(Response::new(EmptyResponse {})); }
        self.controller
            .sender
            .send(WindowAction::Hide)
            .await
            .map_err(|_| Status::unavailable("Candidate UI stopped"))?;
        Ok(Response::new(EmptyResponse {}))
    }
    async fn set_window_position(
        &self,
        request: Request<SetPositionRequest>,
    ) -> Result<Response<EmptyResponse>, Status> {
        if !self.allowed(&request, Kind::Change) { return Ok(Response::new(EmptyResponse {})); }
        let position = request.into_inner().position.ok_or_else(|| Status::invalid_argument("Position required"))?;
        if [position.top, position.left, position.bottom, position.right].iter().any(|v| !(-1_000_000..=1_000_000).contains(v)) {
            return Err(Status::invalid_argument("Position out of range"));
        }
        let top = position.top;
        let left = position.left;
        let bottom = position.bottom;
        let right = position.right;
        self.controller
            .sender
            .send(WindowAction::SetPosition {
                top,
                left,
                bottom,
                right,
            })
            .await
            .map_err(|_| Status::unavailable("Candidate UI stopped"))?;

        Ok(Response::new(EmptyResponse {}))
    }

    async fn set_candidate(
        &self,
        request: Request<SetCandidateRequest>,
    ) -> Result<Response<EmptyResponse>, Status> {
        if !self.allowed(&request, Kind::Change) { return Ok(Response::new(EmptyResponse {})); }
        let request = request.into_inner();
        let candidate = request.candidates;
        if candidate.len() > 1024 || candidate.iter().any(|text| text.len() > 8192)
            || candidate.iter().map(String::len).sum::<usize>() > 65536 || request.note.len() > 1024 {
            return Err(Status::resource_exhausted("Candidate display limit exceeded"));
        }

        self.controller
            .sender
            .send(WindowAction::SetCandidate {
                candidates: candidate,
                note: request.note,
            })
            .await
            .map_err(|_| Status::unavailable("Candidate UI stopped"))?;

        Ok(Response::new(EmptyResponse {}))
    }

    async fn set_selection(
        &self,
        request: Request<SetSelectionRequest>,
    ) -> Result<Response<EmptyResponse>, Status> {
        if !self.allowed(&request, Kind::Change) { return Ok(Response::new(EmptyResponse {})); }
        let index = request.into_inner().index;
        if !(-1..1024).contains(&index) { return Err(Status::invalid_argument("Selection out of range")); }
        self.controller
            .sender
            .send(WindowAction::SetSelection { index })
            .await
            .map_err(|_| Status::unavailable("Candidate UI stopped"))?;

        Ok(Response::new(EmptyResponse {}))
    }

    async fn set_input_mode(
        &self,
        request: Request<SetInputModeRequest>,
    ) -> Result<Response<EmptyResponse>, Status> {
        if !self.allowed(&request, Kind::Change) { return Ok(Response::new(EmptyResponse {})); }
        let request = request.into_inner();
        let mode = request.mode;
        if !["A", "あ"].contains(&mode.as_str()) { return Err(Status::invalid_argument("Unknown input mode")); }
        self.controller
            .sender
            .send(WindowAction::SetInputMode(mode, request.ai_paused))
            .await
            .map_err(|_| Status::unavailable("Candidate UI stopped"))?;

        Ok(Response::new(EmptyResponse {}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::Code;

    /// 前面の処理は 100 番とみる
    fn fixture() -> (WindowService, mpsc::Receiver<WindowAction>) {
        let (sender, receiver) = mpsc::channel(32);
        let gate = Arc::new(Gate::with_check(Box::new(|pid| pid == 100)));
        (WindowService { controller: WindowController::new(sender), gate }, receiver)
    }

    /// 今の入力先 (100 番) の処理の、1 番の接続からの要求
    fn owner_req<T>(message: T) -> Request<T> {
        from(1, 100, message)
    }

    fn from<T>(connection: u64, pid: u32, message: T) -> Request<T> {
        let mut request = Request::new(message);
        request.extensions_mut().insert(TestPeer(connection, pid));
        request
    }

    #[tokio::test]
    async fn modes_are_allowlisted() {
        let (service, mut receiver) = fixture();
        for mode in ["A", "あ"] {
            service.set_input_mode(owner_req(SetInputModeRequest { mode: mode.into(), ..Default::default() })).await.unwrap();
            assert!(matches!(receiver.recv().await, Some(WindowAction::SetInputMode(_, false))));
        }
        for mode in ["", "\";alert(1);//", "A\0"] {
            assert_eq!(service.set_input_mode(owner_req(SetInputModeRequest { mode: mode.into(), ..Default::default() })).await.unwrap_err().code(), Code::InvalidArgument);
        }
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn missing_and_extreme_position_is_rejected() {
        let (service, _receiver) = fixture();
        for position in [None, Some(shared::proto::WindowPosition { left: i32::MIN, ..Default::default() })] {
            assert_eq!(service.set_window_position(owner_req(SetPositionRequest { position })).await.unwrap_err().code(), Code::InvalidArgument);
        }
    }

    #[tokio::test]
    async fn candidates_and_selection_are_bounded() {
        let (service, _receiver) = fixture();
        for candidates in [vec![String::new(); 1025], vec!["x".repeat(8193)], vec!["x".repeat(8192); 9]] {
            assert_eq!(service.set_candidate(owner_req(SetCandidateRequest { candidates, ..Default::default() })).await.unwrap_err().code(), Code::ResourceExhausted);
        }
        for index in [-2, 1024, i32::MAX] {
            assert_eq!(service.set_selection(owner_req(SetSelectionRequest { index })).await.unwrap_err().code(), Code::InvalidArgument);
        }
    }

    #[tokio::test]
    async fn stopped_controller_is_an_error_not_a_panic() {
        let (service, receiver) = fixture();
        drop(receiver);
        assert_eq!(service.show_window(owner_req(EmptyResponse {})).await.unwrap_err().code(), Code::Unavailable);
        assert_eq!(service.set_input_mode(owner_req(SetInputModeRequest { mode: "A".into(), ..Default::default() })).await.unwrap_err().code(), Code::Unavailable);
    }

    /// 2026-11-23 (点検 Astra 02): 今の入力先でない処理からの表示・隠す・候補・位置・入力モードの要求は、何も起こさない (Ok を返して捨てる)
    #[tokio::test]
    async fn requests_from_a_client_that_is_not_the_input_target_do_nothing() {
        let (service, mut receiver) = fixture();
        service.show_window(owner_req(EmptyResponse {})).await.unwrap();
        assert!(matches!(receiver.recv().await, Some(WindowAction::Show)));
        let other = |message| from(2, 200, message);
        service.show_window(other(EmptyResponse {})).await.unwrap();
        service.hide_window(other(EmptyResponse {})).await.unwrap();
        service.set_window_position(from(2, 200, SetPositionRequest { position: Some(Default::default()) })).await.unwrap();
        service.set_candidate(from(2, 200, SetCandidateRequest { candidates: vec!["偽".into()], ..Default::default() })).await.unwrap();
        service.set_selection(from(2, 200, SetSelectionRequest { index: 0 })).await.unwrap();
        service.set_input_mode(from(2, 200, SetInputModeRequest { mode: "A".into(), ..Default::default() })).await.unwrap();
        assert!(receiver.try_recv().is_err(), "a background client changed the candidate window");
        assert_eq!(service.gate.rejected(), 6);
        // 処理の番号が無い接続 (相手を確かめられなかったもの) も同じ
        service.set_candidate(Request::new(SetCandidateRequest { candidates: vec!["偽".into()], ..Default::default() })).await.unwrap();
        assert!(receiver.try_recv().is_err());
    }

    /// 前面が移ったあとでも、最後に窓を出した接続は自分の窓を隠せる。ほかの接続は隠せない
    #[tokio::test]
    async fn the_last_client_that_showed_the_window_may_hide_it() {
        let (service, mut receiver) = fixture();
        service.show_window(owner_req(EmptyResponse {})).await.unwrap();
        assert!(matches!(receiver.recv().await, Some(WindowAction::Show)));
        service.hide_window(from(3, 300, EmptyResponse {})).await.unwrap();
        assert!(receiver.try_recv().is_err());
        service.hide_window(from(1, 999, EmptyResponse {})).await.unwrap();
        assert!(matches!(receiver.recv().await, Some(WindowAction::Hide)));
    }
}
