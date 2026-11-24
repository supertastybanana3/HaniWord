// 変換サーバーに IME と同じ呼び出しを送る動作確認
use hyper_util::rt::TokioIo;
use shared::proto::{azookey_service_client::AzookeyServiceClient, edit_text_request::Op};
use std::time::{Duration, Instant};
use tokio::net::windows::named_pipe::ClientOptions;
use tonic::transport::Endpoint;
use tower::service_fn;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let channel = Endpoint::try_from("http://[::]:50051")?
        .connect_with_connector(service_fn(|_| async {
            let client = ClientOptions::new().open(r"\\.\pipe\haniword_server")?;
            Ok::<_, std::io::Error>(TokioIo::new(client))
        }))
        .await?;
    let mut c = AzookeyServiceClient::new(channel);

    c.clear_text(shared::proto::ClearTextRequest {}).await?;
    let mut last = String::new();
    for ch in "kyouhaiitenkidesune".chars() {
        let r = c
            .edit_text(shared::proto::EditTextRequest { op: Op::Insert as i32, text: ch.to_string(), offset: 0 })
            .await?
            .into_inner();
        last = r.hiragana;
    }
    println!("reading: {last}");
    let t = Instant::now();
    let r = c
        .convert(shared::proto::ConvertRequest { hiragana: last.clone(), context: String::new(), fast: true, dictionary_only: true })
        .await?
        .into_inner();
    println!(
        "convert ({:?}): {}",
        t.elapsed(),
        r.segments.iter().map(|s| format!("{}({})", s.text, s.length)).collect::<Vec<_>>().join(" / ")
    );
    let first = r.segments[0].clone();
    let reading: String = last.chars().take(first.length as usize).collect();
    let r2 = c
        .list_candidates(shared::proto::ListCandidatesRequest { hiragana: reading.clone(), context: String::new() })
        .await?
        .into_inner();
    println!("candidates for {reading}: {:?}", &r2.candidates[..r2.candidates.len().min(8)]);
    let r3 = c
        .learn(shared::proto::LearnRequest { readings: vec![reading], texts: vec![first.text], context: String::new() })
        .await?
        .into_inner();
    println!("learn queued: {}", r3.learned);
    // 同時に何本も投げても落ちないか
    let mut handles = vec![];
    for i in 0..8 {
        let mut c = c.clone();
        handles.push(tokio::spawn(async move {
            c.convert(shared::proto::ConvertRequest { hiragana: "かがくのじゅぎょう".into(), context: String::new(), fast: false, dictionary_only: false })
                .await
                .map(|r| format!("{i}: {}", r.into_inner().segments.iter().map(|s| s.text.clone()).collect::<String>()))
        }));
    }
    for h in handles {
        println!("{}", h.await??);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    c.clear_text(shared::proto::ClearTextRequest {}).await?;
    println!("OK");
    Ok(())
}
