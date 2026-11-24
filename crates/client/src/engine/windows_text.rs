use std::{
    cell::RefCell,
    time::{Duration, Instant},
};
use windows::{
    core::HSTRING,
    Data::Text::{TextConversionGenerator, TextPredictionGenerator},
    Foundation::{AsyncStatus, Collections::IVectorView, IAsyncOperation},
};

const QUERY_TIMEOUT: Duration = Duration::from_millis(300);

thread_local! {
    static CONVERTER: RefCell<Option<TextConversionGenerator>> = const { RefCell::new(None) };
    static PREDICTOR: RefCell<Option<TextPredictionGenerator>> = const { RefCell::new(None) };
}

pub(super) fn conversions(reading: &str) -> Vec<String> {
    conversions_answered(reading).unwrap_or_default()
}

/// 2026-10-22: Windows の変換の候補。時間内に答えが来なかった (または失敗した) ら None。
/// 呼ぶ側は None の一覧を「仮」にして、次に候補を動かしたときに問い直す (同期などで PC が重いと、最初の 1 回が 300ms を超える)
pub(super) fn conversions_answered(reading: &str) -> Option<Vec<String>> {
    let started = Instant::now();
    let operation = CONVERTER.with(|cached| {
        let mut cached = cached.borrow_mut();
        if cached.is_none() {
            *cached = Some(TextConversionGenerator::Create(&HSTRING::from("ja"))?);
        }
        cached
            .as_ref()
            .unwrap()
            .GetCandidatesWithMaxCountAsync(&HSTRING::from(reading), 60)
    });
    finish_query(operation, started)
}

pub(super) fn predictions(reading: &str) -> Vec<String> {
    let started = Instant::now();
    let operation = PREDICTOR.with(|cached| {
        let mut cached = cached.borrow_mut();
        if cached.is_none() {
            *cached = Some(TextPredictionGenerator::Create(&HSTRING::from("ja"))?);
        }
        cached
            .as_ref()
            .unwrap()
            .GetCandidatesWithMaxCountAsync(&HSTRING::from(reading), 20)
    });
    finish_query(operation, started).unwrap_or_default()
}

/// 答えの一覧。時間切れ・失敗は None (答えが空なのとは分ける)
fn finish_query(
    operation: windows::core::Result<IAsyncOperation<IVectorView<HSTRING>>>,
    started: Instant,
) -> Option<Vec<String>> {
    let Ok(operation) = operation else {
        return None;
    };
    let results = (|| -> windows::core::Result<Option<Vec<String>>> {
        loop {
            let remaining = QUERY_TIMEOUT.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                let _ = operation.Cancel();
                return Ok(None);
            }
            match operation.Status()? {
                AsyncStatus::Completed => {
                    let candidates = operation.GetResults()?;
                    return (0..candidates.Size()?)
                        .map(|index| candidates.GetAt(index).map(|text| text.to_string_lossy()))
                        .collect::<windows::core::Result<Vec<String>>>()
                        .map(Some);
                }
                AsyncStatus::Started => {
                    // Do not pump host-window messages or wait indefinitely on the UI thread.
                    std::thread::sleep(remaining.min(Duration::from_millis(2)));
                }
                _ => return Ok(None),
            }
        }
    })();
    if results.is_err() {
        let _ = operation.Cancel();
    }
    results.ok().flatten()
}
