//! RFC 6455 WebSocket 客户端(客户端帧必须掩码;服务端帧不掩码)。
//!
//! 只覆盖 CDP 需要的面:文本帧收发、ping/pong、close、分片续帧。
//! 帧编解码([`FrameCodec`])与传输([`WsConn`])分层,便于纯逻辑单测。
//! 握手接受校验按宽松处理(本地受信浏览器端点,无中间人面)。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::b64;

/// 一条完整消息(已合并分片)。
pub struct WsMessage {
    pub opcode: u8, // 1 = text, 2 = binary
    pub payload: Vec<u8>,
}

impl WsMessage {
    pub fn text(&self) -> Result<&str, String> {
        std::str::from_utf8(&self.payload).map_err(|e| e.to_string())
    }
}

/// 纯帧编解码:喂字节 → 吐完整消息;自动回 pong、拒绝 close。
pub struct FrameCodec {
    buf: Vec<u8>,
    fragments: Vec<u8>,
    frag_opcode: u8, // 0 = 无分片在途
}

pub enum FrameOutcome {
    None,
    Message(WsMessage),
    Pong(Vec<u8>), // 已在内部排队 pong 由 WsConn 发送
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameCodec {
    pub fn new() -> Self {
        FrameCodec {
            buf: Vec::with_capacity(64 * 1024),
            fragments: Vec::new(),
            frag_opcode: 0,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// 大帧(截图 MB 级)预分配提示。
    pub fn needs(&self) -> usize {
        if self.buf.len() < 2 {
            return 2;
        }
        let b1 = self.buf[1];
        let len7 = (b1 & 0x7F) as usize;
        let mask_len = if b1 & 0x80 != 0 { 4 } else { 0 };
        2 + if len7 < 126 {
            mask_len
        } else if len7 == 126 {
            2 + mask_len
        } else {
            8 + mask_len
        }
    }

    pub fn header_len_known(&self) -> bool {
        if self.buf.len() < 2 {
            return false;
        }
        self.buf.len() >= self.needs()
    }

    /// 握手期:查看原始缓冲(HTTP 升级响应头还在缓冲里)。
    pub fn raw(&self) -> &[u8] {
        &self.buf
    }

    /// 握手期:丢弃缓冲前 n 字节(头部已消费)。
    pub fn consume(&mut self, n: usize) {
        let rest = self.buf.split_off(n);
        self.buf = rest;
    }

    /// 尝试解析一帧;控制帧 ping 由调用方(持有传输层)回 pong。
    pub fn try_parse(&mut self) -> Result<FrameOutcome, String> {
        loop {
            if self.buf.len() < 2 {
                return Ok(FrameOutcome::None);
            }
            let b0 = self.buf[0];
            let b1 = self.buf[1];
            let fin = b0 & 0x80 != 0;
            let opcode = b0 & 0x0F;
            let masked = b1 & 0x80 != 0;
            let len7 = (b1 & 0x7F) as usize;
            let mask_len = if masked { 4 } else { 0 };
            let need = 2 + if len7 < 126 {
                mask_len
            } else if len7 == 126 {
                2 + mask_len
            } else {
                8 + mask_len
            };
            if self.buf.len() < need {
                return Ok(FrameOutcome::None);
            }
            let payload_len = if len7 < 126 {
                len7
            } else if len7 == 126 {
                u16::from_be_bytes([self.buf[2], self.buf[3]]) as usize
            } else {
                let mut l = [0u8; 8];
                l.copy_from_slice(&self.buf[2..10]);
                u64::from_be_bytes(l) as usize
            };
            if self.buf.len() < need + payload_len {
                if self.buf.capacity() < need + payload_len {
                    self.buf.reserve(need + payload_len - self.buf.len());
                }
                return Ok(FrameOutcome::None);
            }
            let mask: [u8; 4] = if masked {
                [
                    self.buf[need - 4],
                    self.buf[need - 3],
                    self.buf[need - 2],
                    self.buf[need - 1],
                ]
            } else {
                [0; 4]
            };
            let mut payload: Vec<u8> = self.buf[need..need + payload_len].to_vec();
            if masked {
                for (i, b) in payload.iter_mut().enumerate() {
                    *b ^= mask[i % 4];
                }
            }
            let rest = self.buf.split_off(need + payload_len);
            self.buf = rest;

            match opcode {
                0x9 => return Ok(FrameOutcome::Pong(payload)), // ping(payload 带给传输层回 pong)
                0xA => continue,                               // pong 忽略,继续解析
                0x8 => return Err("WS 对端发送 close".into()),
                0x0..=0x2 => {}
                other => return Err(format!("WS 未知 opcode {other}")),
            }

            if opcode != 0x0 {
                self.fragments = payload;
                self.frag_opcode = opcode;
            } else {
                self.fragments.extend_from_slice(&payload);
            }
            if fin {
                let opcode = self.frag_opcode;
                self.frag_opcode = 0;
                let payload = std::mem::take(&mut self.fragments);
                return Ok(FrameOutcome::Message(WsMessage { opcode, payload }));
            }
            // 分片未完,继续解析下一帧
        }
    }
}

fn build_client_frame(fin: bool, opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut head = Vec::with_capacity(10 + payload.len());
    head.push((if fin { 0x80 } else { 0 }) | opcode);
    let len = payload.len();
    if len < 126 {
        head.push(0x80 | len as u8);
    } else if len <= u16::MAX as usize {
        head.push(0x80 | 126);
        head.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        head.push(0x80 | 127);
        head.extend_from_slice(&(len as u64).to_be_bytes());
    }
    head.extend_from_slice(&mask);
    head.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    head
}

pub struct WsConn {
    stream: TcpStream,
    codec: FrameCodec,
    next_mask_seed: u8,
    /// ping 帧的 payload 备查;当前传输层直接回 pong,不消费(协议备忘)。
    #[allow(dead_code)]
    pong_queue: Vec<Vec<u8>>,
}

impl WsConn {
    /// 连接 ws://host:port/path 并完成升级握手。
    pub fn connect(host: &str, port: u16, path: &str, timeout: Duration) -> Result<Self, String> {
        let stream = TcpStream::connect((host, port))
            .map_err(|e| format!("WS 连接 {host}:{port} 失败: {e}"))?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .ok();
        stream.set_write_timeout(Some(timeout)).ok();
        stream.set_nodelay(true).ok();

        // Sec-WebSocket-Key:16 字节伪随机 → base64(握手键非安全凭据,弱随机足够)
        let mut key_bytes = [0u8; 16];
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            ^ ((std::process::id() as u64) << 32);
        for (i, b) in key_bytes.iter_mut().enumerate() {
            // 全程 wrapping:i×常数在 i≥13 时溢出 u64——release 默认关闭
            // 溢出检查掩盖了此点,debug 构建(ci.ps1 门禁 3/e2e)panic
            let x = (seed)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add((i as u64).wrapping_mul(0xBF58476D1CE4E5B9));
            *b = (x >> 24) as u8 ^ (x >> 8) as u8;
        }
        let key = b64::encode(&key_bytes);
        let req = format!(
            "GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        let mut conn = WsConn {
            stream,
            codec: FrameCodec::new(),
            next_mask_seed: 0x37,
            pong_queue: Vec::new(),
        };
        conn.stream
            .write_all(req.as_bytes())
            .map_err(|e| format!("WS 握手写入失败: {e}"))?;
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() > deadline {
                return Err("WS 握手超时".into());
            }
            if let Some(header_end) = conn.codec.raw().windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&conn.codec.raw()[..header_end]).to_string();
                conn.codec.consume(header_end + 4);
                if !head.starts_with("HTTP/1.1 101") {
                    return Err(format!(
                        "WS 握手被拒: {}",
                        head.lines().next().unwrap_or("")
                    ));
                }
                return Ok(conn);
            }
            if !conn.read_some()? {
                return Err("WS 握手期间连接关闭".into());
            }
        }
    }

    fn next_mask(&mut self) -> [u8; 4] {
        let m = self.next_mask_seed;
        self.next_mask_seed = self.next_mask_seed.wrapping_mul(31).wrapping_add(7);
        [m, m.wrapping_add(1), m.wrapping_add(2), m.wrapping_add(3)]
    }

    /// 发送文本帧(>32KB 自动分片)。
    pub fn send_text(&mut self, text: &str) -> Result<(), String> {
        let payload = text.as_bytes();
        if payload.is_empty() {
            return self.send_frame(true, 0x1, &[]);
        }
        let mut first = true;
        for (i, chunk) in payload.chunks(32 * 1024).enumerate() {
            let last = (i + 1) * 32 * 1024 >= payload.len();
            let fin = last;
            let opcode = if first { 0x1 } else { 0x0 };
            self.send_frame(fin, opcode, chunk)?;
            first = false;
        }
        Ok(())
    }

    fn send_frame(&mut self, fin: bool, opcode: u8, payload: &[u8]) -> Result<(), String> {
        let mask = self.next_mask();
        let frame = build_client_frame(fin, opcode, payload, mask);
        self.stream
            .write_all(&frame)
            .map_err(|e| format!("WS 帧写入失败: {e}"))?;
        self.stream.flush().ok();
        Ok(())
    }

    /// 从 socket 尽量读一段喂 codec。Ok(false) = 对端关闭。
    fn read_some(&mut self) -> Result<bool, String> {
        let mut chunk = [0u8; 64 * 1024];
        match self.stream.read(&mut chunk) {
            Ok(0) => Ok(false),
            Ok(n) => {
                self.codec.feed(&chunk[..n]);
                Ok(true)
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                Ok(true)
            }
            Err(e) => Err(format!("WS 读取失败: {e}")),
        }
    }

    /// 轮询一条完整消息(非阻塞:受 socket read_timeout 界定,无数据立即返回)。
    /// Ok(None) = 暂无完整消息(不丢已收半包)。**不得**在内部循环等待——
    /// 否则上层所有 deadline 都会被架空。
    pub fn poll_message(&mut self) -> Result<Option<WsMessage>, String> {
        match self.codec.try_parse()? {
            FrameOutcome::Message(m) => return Ok(Some(m)),
            FrameOutcome::Pong(p) => {
                self.send_frame(true, 0xA, &p)?;
            }
            FrameOutcome::None => {}
        }
        if !self.read_some()? {
            return Ok(None); // 对端关闭
        }
        match self.codec.try_parse()? {
            FrameOutcome::Message(m) => Ok(Some(m)),
            FrameOutcome::Pong(p) => {
                self.send_frame(true, 0xA, &p)?;
                Ok(None)
            }
            FrameOutcome::None => Ok(None),
        }
    }

    /// 阻塞等待一条完整消息(带总超时)。
    pub fn read_message(&mut self, timeout: Duration) -> Result<WsMessage, String> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.codec.try_parse()? {
                FrameOutcome::Message(m) => return Ok(m),
                FrameOutcome::Pong(p) => {
                    self.send_frame(true, 0xA, &p)?;
                }
                FrameOutcome::None => {}
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("WS 等待消息超时".into());
            }
            self.stream
                .set_read_timeout(Some(remaining.min(Duration::from_millis(250))))
                .ok();
            if !self.read_some()? {
                return Err("WS 等待消息超时(连接关闭)".into());
            }
        }
    }

    pub fn close(&mut self) {
        let _ = self.send_frame(true, 0x8, &[]);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_frame() {
        let mut codec = FrameCodec::new();
        codec.feed(&[0x81, 0x02, b'h', b'i']);
        match codec.try_parse().unwrap() {
            FrameOutcome::Message(m) => {
                assert_eq!(m.opcode, 1);
                assert_eq!(m.payload, b"hi");
            }
            _ => panic!("应为消息"),
        }
        assert!(matches!(codec.try_parse().unwrap(), FrameOutcome::None));
    }

    #[test]
    fn parse_extended_length() {
        let mut codec = FrameCodec::new();
        let payload = vec![0xABu8; 300];
        let mut frame = vec![0x82, 126];
        frame.extend_from_slice(&(300u16).to_be_bytes());
        frame.extend_from_slice(&payload);
        codec.feed(&frame);
        match codec.try_parse().unwrap() {
            FrameOutcome::Message(m) => {
                assert_eq!(m.opcode, 2);
                assert_eq!(m.payload.len(), 300);
            }
            _ => panic!("应为消息"),
        }
    }

    #[test]
    fn fragmented_text() {
        let mut codec = FrameCodec::new();
        codec.feed(&[0x01, 0x01, b'a']); // 首片(opcode=1,FIN=0)
        assert!(matches!(codec.try_parse().unwrap(), FrameOutcome::None));
        codec.feed(&[0x80, 0x02, b'b', b'c']); // 续片(opcode=0,FIN=1)
        match codec.try_parse().unwrap() {
            FrameOutcome::Message(m) => assert_eq!(m.payload, b"abc"),
            _ => panic!("应为合并消息"),
        }
    }

    #[test]
    fn partial_arrival_keeps_state() {
        let mut codec = FrameCodec::new();
        let payload = vec![7u8; 1000];
        let mut frame = vec![0x82, 126];
        frame.extend_from_slice(&(1000u16).to_be_bytes());
        frame.extend_from_slice(&payload);
        codec.feed(&frame[..5]);
        assert!(matches!(codec.try_parse().unwrap(), FrameOutcome::None));
        codec.feed(&frame[5..600]);
        assert!(matches!(codec.try_parse().unwrap(), FrameOutcome::None));
        codec.feed(&frame[600..]);
        match codec.try_parse().unwrap() {
            FrameOutcome::Message(m) => assert_eq!(m.payload.len(), 1000),
            _ => panic!("应为消息"),
        }
    }

    #[test]
    fn client_frame_mask_roundtrip() {
        let payload = b"hello world";
        let frame = build_client_frame(true, 0x1, payload, [1, 2, 3, 4]);
        let mut codec = FrameCodec::new();
        codec.feed(&frame);
        match codec.try_parse().unwrap() {
            FrameOutcome::Message(m) => assert_eq!(m.payload, payload),
            _ => panic!("应为消息"),
        }
    }

    #[test]
    fn ping_yields_pong_outcome() {
        let mut codec = FrameCodec::new();
        codec.feed(&[0x89, 0x00]); // 服务端 ping 无 payload
        assert!(matches!(codec.try_parse().unwrap(), FrameOutcome::Pong(_)));
    }
}
