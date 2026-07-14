use tokio::sync::mpsc::UnboundedReceiver;

use crate::{error::Result, response::Usage};

/// One item from a streaming response.
#[derive(Clone, Debug)]
pub enum StreamChunk {
    /// A piece of generated text.
    Delta(String),
    /// The stream finished; carries final token usage.
    Done(Usage),
}

/// A live text stream. Pull chunks with [`next`](TextStream::next), or drain the
/// whole thing with [`collect_text`](TextStream::collect_text).
///
/// ```no_run
/// # async fn demo(ai: elyra_ai::Ai) -> elyra_ai::Result<()> {
/// use elyra_ai::StreamChunk;
/// let mut stream = ai.chat().instructions("Be brief.").stream("Hello");
/// while let Some(chunk) = stream.next().await {
///     match chunk? {
///         StreamChunk::Delta(text) => print!("{text}"),
///         StreamChunk::Done(usage) => eprintln!("\n{usage:?}"),
///     }
/// }
/// # Ok(()) }
/// ```
pub struct TextStream {
    pub(crate) rx: UnboundedReceiver<Result<StreamChunk>>,
}

impl TextStream {
    /// The next chunk, or `None` when the stream ends.
    pub async fn next(&mut self) -> Option<Result<StreamChunk>> {
        self.rx.recv().await
    }

    /// Consume the stream, concatenating every text delta into the final string.
    pub async fn collect_text(mut self) -> Result<String> {
        let mut out = String::new();
        while let Some(item) = self.rx.recv().await {
            if let StreamChunk::Delta(text) = item? {
                out.push_str(&text);
            }
        }
        Ok(out)
    }
}
