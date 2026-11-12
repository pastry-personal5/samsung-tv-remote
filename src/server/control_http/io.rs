//! Bound each response flush, including a peer that makes only partial progress.
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::time::{sleep, Sleep};

pub(super) struct WriteDeadline<T = TcpStream> {
    stream: T,
    stalled: Option<Pin<Box<Sleep>>>,
    header: Vec<u8>,
    audited: bool,
    invalid_framing: Arc<AtomicBool>,
}
impl<T> WriteDeadline<T> {
    pub fn new(stream: T, invalid_framing: Arc<AtomicBool>) -> Self {
        Self {
            stream,
            stalled: None,
            header: Vec::new(),
            audited: false,
            invalid_framing,
        }
    }
    fn audit(&mut self, bytes: &[u8]) {
        // Hyper is the sole framing parser. It deliberately normalizes duplicate
        // lengths and CL+TE, so retain only a rejection bit for those raw header
        // combinations. This audit neither decodes bodies nor chooses framing.
        if self.audited {
            return;
        }
        for byte in bytes {
            if self.header.len() == super::transport::MAX_HEADERS {
                self.invalid_framing.store(true, Ordering::Release);
                self.audited = true;
                self.header.clear();
                break;
            }
            self.header.push(*byte);
            if self.header.ends_with(b"\r\n\r\n") {
                let mut lengths = 0;
                let mut encodings = 0;
                for line in self.header.split(|v| *v == b'\n').skip(1) {
                    let Some(end) = line.iter().position(|v| *v == b':') else {
                        continue;
                    };
                    let name = &line[..end];
                    if name.eq_ignore_ascii_case(b"content-length") {
                        lengths += 1;
                    }
                    if name.eq_ignore_ascii_case(b"transfer-encoding") {
                        encodings += 1;
                    }
                }
                self.invalid_framing.store(
                    lengths > 1 || encodings > 1 || (lengths > 0 && encodings > 0),
                    Ordering::Release,
                );
                self.audited = true;
                self.header.clear();
                break;
            }
        }
    }
    fn pending(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let timer = self
            .stalled
            .get_or_insert_with(|| Box::pin(sleep(Duration::from_secs(10))));
        if timer.as_mut().poll(cx).is_ready() {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "response write deadline",
            )))
        } else {
            Poll::Pending
        }
    }
}
impl<T: AsyncRead + Unpin> AsyncRead for WriteDeadline<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        if result.is_ready() {
            self.audit(&buf.filled()[before..]);
        }
        result
    }
}
impl<T: AsyncWrite + Unpin> AsyncWrite for WriteDeadline<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Keep one absolute deadline until Hyper flushes this output batch.
        // Resetting after each successful short write lets a trickling reader
        // retain a large SSE frame indefinitely.
        if let Poll::Ready(Err(error)) = self.pending(cx) {
            return Poll::Ready(Err(error));
        }
        match Pin::new(&mut self.stream).poll_write(cx, buf) {
            Poll::Ready(result) => Poll::Ready(result),
            Poll::Pending => self.pending(cx).map_ok(|()| 0),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.stalled.is_some() {
            if let Poll::Ready(Err(error)) = self.pending(cx) {
                return Poll::Ready(Err(error));
            }
        }
        match Pin::new(&mut self.stream).poll_flush(cx) {
            Poll::Ready(result) => {
                self.stalled = None;
                Poll::Ready(result)
            }
            Poll::Pending => self.pending(cx),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    struct Blocked;
    impl AsyncWrite for Blocked {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Pending
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    #[tokio::test(start_paused = true)]
    async fn abandoned_response_writer_has_a_finite_deadline() {
        let mut io = WriteDeadline::new(Blocked, Arc::new(AtomicBool::new(false)));
        let start = tokio::time::Instant::now();
        assert_eq!(
            io.write_all(b"response").await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }

    struct Trickling(Pin<Box<Sleep>>);
    impl AsyncWrite for Trickling {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            if self.0.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.0
                .as_mut()
                .reset(tokio::time::Instant::now() + Duration::from_secs(6));
            Poll::Ready(Ok(1))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }
    #[tokio::test(start_paused = true)]
    async fn partial_progress_cannot_extend_the_flush_deadline() {
        let stream = Trickling(Box::pin(sleep(Duration::from_secs(6))));
        let mut io = WriteDeadline::new(stream, Arc::new(AtomicBool::new(false)));
        let start = tokio::time::Instant::now();
        assert_eq!(
            io.write_all(b"frame").await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }
    #[tokio::test(start_paused = true)]
    async fn completed_flush_allows_an_idle_stream_to_write_again() {
        let mut io = WriteDeadline::new(tokio::io::sink(), Arc::new(AtomicBool::new(false)));
        io.write_all(b"snapshot").await.unwrap();
        io.flush().await.unwrap();
        tokio::time::advance(Duration::from_secs(30)).await;
        io.write_all(b"heartbeat").await.unwrap();
        io.flush().await.unwrap();
    }
}
