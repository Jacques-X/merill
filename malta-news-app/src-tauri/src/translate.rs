use anyhow::{Context, Result};

use crate::models::RawArticle;

/// Translate text via Google Translate's free endpoint, with up to 3 retries.
pub async fn translate_text(client: &reqwest::Client, text: &str, from: &str, to: &str) -> Result<String> {
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0u32..3 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(500 * (1u64 << attempt))).await;
        }
        match try_translate(client, text, from, to).await {
            Ok(r) => return Ok(r),
            Err(e) => {
                log::debug!("translate attempt {} failed: {}", attempt + 1, e);
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap())
}

async fn try_translate(client: &reqwest::Client, text: &str, from: &str, to: &str) -> Result<String> {
    let resp = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", from),
            ("tl", to),
            ("dt", "t"),
            ("q", text),
        ])
        .send()
        .await
        .context("Google Translate request failed")?
        .error_for_status()
        .context("Google Translate returned error")?
        .text()
        .await?;

    let parsed: serde_json::Value = serde_json::from_str(&resp)
        .context("failed to parse Google Translate response")?;

    let mut result = String::new();
    if let Some(segments) = parsed.get(0).and_then(|v| v.as_array()) {
        for seg in segments {
            if let Some(text) = seg.get(0).and_then(|v| v.as_str()) {
                result.push_str(text);
            }
        }
    }

    if result.is_empty() {
        anyhow::bail!("empty translation result");
    }
    Ok(result)
}

/// Translate a list of (index, headline) pairs. Returns (index, translated) pairs.
/// Preserves chunk-of-10 batching to avoid rate limiting.
async fn translate_tasks(
    client: &reqwest::Client,
    tasks: Vec<(usize, String)>,
    from: &str,
    to: &str,
) -> Vec<(usize, String)> {
    let mut results = Vec::with_capacity(tasks.len());
    for chunk in tasks.chunks(10) {
        let futs: Vec<_> = chunk
            .iter()
            .map(|(i, headline)| {
                let client = client.clone();
                let headline = headline.clone();
                let from = from.to_string();
                let to = to.to_string();
                let i = *i;
                async move {
                    let translated = translate_text(&client, &headline, &from, &to)
                        .await
                        .unwrap_or_else(|e| {
                            log::warn!("translation failed for {:?}: {}", &headline, e);
                            headline.clone()
                        });
                    (i, translated)
                }
            })
            .collect();
        results.extend(futures::future::join_all(futs).await);
    }
    results
}

/// Translate all article headlines to the other language.
/// MT→EN and EN→MT run concurrently. Articles with a non-empty translated_headline are skipped.
pub async fn translate_headlines(articles: &mut [RawArticle]) {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    // Snapshot (index, headline) pairs — releases immutable borrow before we write results back
    let mt_tasks: Vec<(usize, String)> = articles
        .iter()
        .enumerate()
        .filter(|(_, a)| a.language == "mt" && a.translated_headline.is_empty())
        .map(|(i, a)| (i, a.original_headline.clone()))
        .collect();

    // Only translate MT→EN: needed for clustering and consistent display.
    // EN→MT is skipped — the UI falls back to the original English headline.
    if mt_tasks.is_empty() {
        return;
    }
    log::info!("translating {} mt->en headlines", mt_tasks.len());

    let mt_results = translate_tasks(&client, mt_tasks, "mt", "en").await;

    let translated_count = mt_results.iter()
        .filter(|(i, t)| t != &articles[*i].original_headline)
        .count();
    let total = mt_results.len();

    for (idx, t) in mt_results {
        articles[idx].translated_headline = t;
    }

    log::info!("translation complete: {}/{} mt->en headlines translated", translated_count, total);
}
