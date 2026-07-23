//! Simulated Rewave receiver — executable spec for sender-side integration tests.
//! Binds UDP 127.0.0.1:{audio_port, disc_port}; speaks the full §5/§6/§7 protocol.
#![allow(dead_code)]

use rewave_core::crypto::{ecdh, session};
use rewave_core::protocol::control::{
    decode, encode, encode_header, ControlMessage, TYPE_CHALLENGE,
};
use rewave_core::protocol::datagram::{decode_m1, decode_m6};
use rewave_core::protocol::dispatch::{Class, Dispatcher};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

type PcmSink = Arc<Mutex<Vec<(u32, Vec<u8>)>>>;

#[derive(Default)]
pub struct SimStats {
    pub auth_failures: u64,
    pub replay_drops: u64,
    pub accepted: u64,
    pub pair_requests: u64,
}

#[derive(Default)]
struct Shared {
    challenge_nonce: Option<[u8; 16]>,
    pairings: HashMap<[u8; 8], [u8; 32]>,
    pairing_key: Option<[u8; 32]>,
    key_id: Option<[u8; 8]>,
    last_session_key: Option<[u8; 32]>,
    session_key: Option<[u8; 32]>,
}

pub struct SimReceiver {
    pub audio_port: u16,
    pub disc_port: u16,
    pub name: String,
    pub pin: String,
    pub pcm_sink: PcmSink,
    pub stats: Arc<Mutex<SimStats>>,
    shared: Arc<Mutex<Shared>>,
    shutdown: Arc<AtomicBool>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl SimReceiver {
    pub fn start(name: String, pin: String) -> Self {
        let disc_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let audio_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        disc_sock
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        audio_sock
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let disc_port = disc_sock.local_addr().unwrap().port();
        let audio_port = audio_sock.local_addr().unwrap().port();

        let shared = Arc::new(Mutex::new(Shared::default()));
        let stats = Arc::new(Mutex::new(SimStats::default()));
        let pcm_sink = Arc::new(Mutex::new(Vec::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let t_disc = {
            let (shared, stats, shutdown) = (shared.clone(), stats.clone(), shutdown.clone());
            let name = name.clone();
            std::thread::spawn(move || disc_loop(disc_sock, name, audio_port, shared, stats, shutdown))
        };
        let t_audio = {
            let (shared, stats, pcm_sink, shutdown) =
                (shared.clone(), stats.clone(), pcm_sink.clone(), shutdown.clone());
            let pin = pin.clone();
            std::thread::spawn(move || audio_loop(audio_sock, pin, shared, stats, pcm_sink, shutdown))
        };

        Self {
            audio_port,
            disc_port,
            name,
            pin,
            pcm_sink,
            stats,
            shared,
            shutdown,
            threads: Mutex::new(vec![t_disc, t_audio]),
        }
    }

    pub fn pairing_key(&self) -> Option<[u8; 32]> {
        self.shared.lock().unwrap().pairing_key
    }

    pub fn key_id(&self) -> Option<[u8; 8]> {
        self.shared.lock().unwrap().key_id
    }

    pub fn last_session_key(&self) -> Option<[u8; 32]> {
        self.shared.lock().unwrap().last_session_key
    }
}

impl Drop for SimReceiver {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        for t in self.threads.lock().unwrap().drain(..) {
            let _ = t.join();
        }
    }
}

fn is_timeout(e: &std::io::Error) -> bool {
    e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut
}

fn send_challenge(sock: &UdpSocket, src: std::net::SocketAddr, shared: &Arc<Mutex<Shared>>, version: u8) {
    let nonce: [u8; 16] = rand::random();
    let salt: [u8; 16] = rand::random();
    shared.lock().unwrap().challenge_nonce = Some(nonce);
    let bytes = if version == 1 {
        let mut b = encode_header(1, TYPE_CHALLENGE, 32).to_vec();
        b.extend_from_slice(&nonce);
        b.extend_from_slice(&salt);
        b
    } else {
        encode(&ControlMessage::Challenge { receiver_nonce: nonce, salt })
    };
    let _ = sock.send_to(&bytes, src);
}

fn disc_loop(
    sock: UdpSocket,
    name: String,
    audio_port: u16,
    shared: Arc<Mutex<Shared>>,
    stats: Arc<Mutex<SimStats>>,
    shutdown: Arc<AtomicBool>,
) {
    let mut buf = [0u8; 2048];
    while !shutdown.load(Ordering::Relaxed) {
        let (n, src) = match sock.recv_from(&mut buf) {
            Ok(x) => x,
            Err(e) if is_timeout(&e) => continue,
            Err(_) => break,
        };
        let Some(msg) = decode(&buf[..n]) else { continue };
        match msg {
            ControlMessage::HelloV1 => {
                let _ = sock.send_to(
                    &encode(&ControlMessage::HereV1 { port: audio_port, ipv4: 0x7F00_0001 }),
                    src,
                );
                send_challenge(&sock, src, &shared, 1);
            }
            ControlMessage::HelloV2 { .. } => {
                let _ = sock.send_to(
                    &encode(&ControlMessage::HereV2 {
                        port: audio_port,
                        ipv4: 0x7F00_0001,
                        name: name.clone(),
                        receiver_flags: 0,
                    }),
                    src,
                );
                send_challenge(&sock, src, &shared, 2);
            }
            ControlMessage::PairRequest { sender_pubkey, sender_nonce, .. } => {
                stats.lock().unwrap().pair_requests += 1;
                let Ok(peer_pub) = ecdh::decode_pubkey(&sender_pubkey) else { continue };
                let (sk, pub65) = ecdh::generate_keypair();
                let nr: [u8; 16] = rand::random();
                let shared_secret = ecdh::ecdh_shared(&sk, &peer_pub);
                let pk = ecdh::derive_pairing_key(&shared_secret, &sender_nonce, &nr);
                let kid = ecdh::derive_key_id(&pk);
                let session_key = ecdh::derive_session_key(&pk, &sender_nonce, &nr);
                {
                    let mut sh = shared.lock().unwrap();
                    sh.pairings.insert(kid, pk);
                    sh.pairing_key = Some(pk);
                    sh.key_id = Some(kid);
                    sh.last_session_key = Some(session_key);
                    sh.session_key = Some(session_key);
                }
                let _ = sock.send_to(
                    &encode(&ControlMessage::PairConfirm {
                        receiver_pubkey: pub65,
                        receiver_nonce: nr,
                        key_id: kid,
                    }),
                    src,
                );
            }
            ControlMessage::PairResume { key_id, sender_nonce, hmac } => {
                let pk = shared.lock().unwrap().pairings.get(&key_id).copied();
                let Some(pk) = pk else { continue };
                if session::resume_sender_hmac(&pk, &key_id, &sender_nonce) != hmac {
                    continue;
                }
                let nr: [u8; 16] = rand::random();
                let session_key = ecdh::derive_session_key(&pk, &sender_nonce, &nr);
                {
                    let mut sh = shared.lock().unwrap();
                    sh.session_key = Some(session_key);
                    sh.last_session_key = Some(session_key);
                }
                let rh = session::resume_receiver_hmac(&pk, &key_id, &sender_nonce, &nr);
                let _ = sock.send_to(
                    &encode(&ControlMessage::PairResumeOk { receiver_nonce: nr, hmac: rh }),
                    src,
                );
            }
            _ => {}
        }
    }
}

fn audio_loop(
    sock: UdpSocket,
    pin: String,
    shared: Arc<Mutex<Shared>>,
    stats: Arc<Mutex<SimStats>>,
    pcm_sink: PcmSink,
    shutdown: Arc<AtomicBool>,
) {
    let mut dispatcher = Dispatcher::new();
    let mut local_key: Option<[u8; 32]> = None;
    let mut buf = [0u8; 2048];
    while !shutdown.load(Ordering::Relaxed) {
        let recv = sock.recv_from(&mut buf);
        {
            let sh = shared.lock().unwrap();
            if sh.session_key != local_key {
                local_key = sh.session_key;
                match local_key {
                    Some(k) => dispatcher.adopt_session(k),
                    None => dispatcher.reset_session(),
                }
            }
        }
        let (n, src) = match recv {
            Ok(x) => x,
            Err(e) if is_timeout(&e) => continue,
            Err(_) => break,
        };
        let d = &buf[..n];
        if d.len() >= 10 && &d[0..6] == b"REWAVE" {
            if let Some(ControlMessage::HandshakeResponse { sender_nonce, response }) = decode(d) {
                let nr = shared.lock().unwrap().challenge_nonce;
                let ok = nr.is_some_and(|nr| {
                    let expected =
                        session::pin_handshake_response(pin.as_bytes(), &sender_nonce, &nr);
                    response == expected || response[..8] == expected[..8]
                });
                if ok {
                    let nr = nr.unwrap();
                    let sk = session::pin_session_key(pin.as_bytes(), &sender_nonce, &nr);
                    dispatcher.adopt_session(sk);
                    local_key = Some(sk);
                    {
                        let mut sh = shared.lock().unwrap();
                        sh.session_key = Some(sk);
                        sh.last_session_key = Some(sk);
                    }
                    let _ = sock.send_to(&encode(&ControlMessage::HandshakeOk), src);
                } else {
                    let _ = sock.send_to(&encode(&ControlMessage::HandshakeFail), src);
                }
            }
            continue;
        }
        match dispatcher.classify(d) {
            Class::AuthCandidate => {
                let key = local_key.unwrap();
                match decode_m6(d, &key) {
                    Some((seq, _flags, pcm)) => {
                        let mut st = stats.lock().unwrap();
                        if dispatcher.check_and_advance_seq(seq) {
                            st.accepted += 1;
                            drop(st);
                            pcm_sink.lock().unwrap().push((seq, pcm));
                        } else {
                            st.replay_drops += 1;
                        }
                    }
                    None => stats.lock().unwrap().auth_failures += 1,
                }
            }
            Class::LegacyAccept => {
                if let Some((seq, _flags, pcm)) = decode_m1(d) {
                    pcm_sink.lock().unwrap().push((seq, pcm));
                }
            }
            _ => {}
        }
    }
}
