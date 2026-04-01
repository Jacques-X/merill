use anyhow::{Context, Result};
use std::collections::HashSet;

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

/// Batch-translate a set of articles (by index) with given language pair.
async fn translate_batch(
    client: &reqwest::Client,
    articles: &mut [RawArticle],
    indices: &[usize],
    from: &str,
    to: &str,
) {
    if indices.is_empty() {
        return;
    }
    log::info!("translating {} headlines ({} -> {})", indices.len(), from, to);

    for chunk in indices.chunks(10) {
        let futs: Vec<_> = chunk
            .iter()
            .map(|&i| {
                let headline = articles[i].original_headline.clone();
                let client = client.clone();
                let from = from.to_string();
                let to = to.to_string();
                async move { translate_text(&client, &headline, &from, &to).await }
            })
            .collect();

        let results = futures::future::join_all(futs).await;
        for (&idx, result) in chunk.iter().zip(results) {
            match result {
                Ok(translated) => {
                    log::debug!(
                        "translated: {:?} -> {:?}",
                        &articles[idx].original_headline,
                        &translated
                    );
                    articles[idx].translated_headline = translated;
                }
                Err(e) => {
                    log::warn!(
                        "translation failed for {:?}: {}",
                        &articles[idx].original_headline,
                        e
                    );
                    // Fallback: use original headline
                    articles[idx].translated_headline =
                        articles[idx].original_headline.clone();
                }
            }
        }
    }
}

/// Translate all article headlines to the other language.
/// - Maltese articles get English translations (for clustering + EN display)
/// - English articles get Maltese translations (for MT display)
pub async fn translate_headlines(articles: &mut [RawArticle]) {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let mt_indices: Vec<usize> = articles
        .iter()
        .enumerate()
        .filter(|(_, a)| a.language == "mt" && a.translated_headline.is_empty())
        .map(|(i, _)| i)
        .collect();

    let en_indices: Vec<usize> = articles
        .iter()
        .enumerate()
        .filter(|(_, a)| a.language == "en" && a.translated_headline.is_empty())
        .map(|(i, _)| i)
        .collect();

    // MT -> EN first (needed for clustering)
    translate_batch(&client, articles, &mt_indices, "mt", "en").await;
    // EN -> MT (for Maltese UI display)
    translate_batch(&client, articles, &en_indices, "en", "mt").await;

    // Use HashSet for O(1) membership checks
    let mt_set: HashSet<usize> = mt_indices.into_iter().collect();
    let en_set: HashSet<usize> = en_indices.into_iter().collect();

    let total = mt_set.len() + en_set.len();
    let translated = articles
        .iter()
        .enumerate()
        .filter(|(i, a)| {
            (mt_set.contains(i) || en_set.contains(i))
                && a.translated_headline != a.original_headline
        })
        .count();
    log::info!(
        "translation complete: {}/{} headlines translated",
        translated,
        total
    );
}
