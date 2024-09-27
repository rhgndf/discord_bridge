use pin_project::pin_project;
use serenity::async_trait;
use songbird::input::{AsyncMediaSource, AudioStreamError};
use std::{
    io::{self, ErrorKind, SeekFrom}, pin::Pin, task::{Context, Poll}
};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};

pub fn extract_callsign(nick: &String) -> Option<String> {
    // Split by spaces, if any tokens are:
    //  Longer than 2 characters
    //  All uppercase including numbers
    //  Ends with a letter
    // Then return callsign
    nick.split_whitespace()
        .filter(|x| {
            x.len() > 2
                && x.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                && x.chars().last().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
        })
        .map(|x| x.to_string())
        .next()
}


#[pin_project]
pub struct RingBufferStream {
    #[pin]
    pub stream: Box<dyn AsyncRead + Send + Sync + Unpin>,
}

impl AsyncRead for RingBufferStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        AsyncRead::poll_read(self.project().stream, cx, buf)
    }
}

impl AsyncSeek for RingBufferStream {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Err(ErrorKind::Unsupported.into())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        unreachable!()
    }
}

#[async_trait]
impl AsyncMediaSource for RingBufferStream {
    fn is_seekable(&self) -> bool {
        false
    }

    async fn byte_len(&self) -> Option<u64> {
        None
    }

    async fn try_resume(
        &mut self,
        _offset: u64,
    ) -> Result<Box<dyn AsyncMediaSource>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }
}