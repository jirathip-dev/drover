use super::wire;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const CONNS: usize = 256;
const HANDLERS: usize = 64;
const READ: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Pool {
    handlers: Arc<Semaphore>,
    jobs: Sender<Job>,
    rt: Handle,
}

/// One request read off its socket, holding one of the 64 handler slots until it is answered.
pub struct Job {
    s: std::net::TcpStream,
    req: Vec<u8>,
    over: bool,
    rest: Vec<u8>,
    conn: OwnedSemaphorePermit,
    handler: OwnedSemaphorePermit,
    pool: Pool,
}

impl Job {
    /// Answer it on this thread (`one` says whether the socket stays open), then give a kept socket back to the runtime.
    pub fn answer(self, one: impl FnOnce(&mut std::net::TcpStream, &[u8], bool) -> bool) {
        let Job {
            mut s,
            req,
            over,
            rest,
            conn,
            handler,
            pool,
        } = self;
        let keep = one(&mut s, &req, over);
        drop(handler);
        if keep && s.set_nonblocking(true).is_ok() {
            let p = pool.clone();
            pool.rt.spawn(async move {
                if let Ok(s) = TcpStream::from_std(s) {
                    read(p, s, rest, conn).await;
                }
            });
        }
    }
}

async fn read(p: Pool, mut s: TcpStream, mut rest: Vec<u8>, conn: OwnedSemaphorePermit) {
    let Ok(Ok((req, over))) = tokio::time::timeout(READ, wire::request(&mut s, &mut rest)).await else {
        return;
    };
    if req.is_empty() {
        return;
    }
    let Ok(handler) = p.handlers.clone().acquire_owned().await else {
        return;
    };
    let Ok(s) = s.into_std() else { return };
    if s.set_nonblocking(false)
        .and_then(|()| s.set_read_timeout(Some(READ)))
        .is_ok()
    {
        let jobs = p.jobs.clone();
        drop(jobs.send(Job {
            s,
            req,
            over,
            rest,
            conn,
            handler,
            pool: p,
        }));
    }
}

/// Accept on `l` on a runtime of 2 workers, reading each request there (5 s for all of it), then sending it to `jobs`.
/// Past 256 open connections one gets `busy` (503); after each accept `each` gets whether every connection is done.
pub fn accept(
    l: std::net::TcpListener,
    jobs: Sender<Job>,
    busy: &[u8],
    full: impl Fn(),
    mut each: impl FnMut(&dyn Fn() -> bool),
) -> std::io::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let conns = Arc::new(Semaphore::new(CONNS));
    let pool = Pool {
        handlers: Arc::new(Semaphore::new(HANDLERS)),
        jobs,
        rt: rt.handle().clone(),
    };
    let idle = || conns.available_permits() == CONNS;
    rt.block_on(async {
        l.set_nonblocking(true)?;
        let l = TcpListener::from_std(l)?;
        loop {
            let Ok((mut s, _)) = l.accept().await else { continue };
            if let Ok(c) = conns.clone().try_acquire_owned() {
                tokio::spawn(read(pool.clone(), s, vec![], c));
            } else {
                full();
                drop(s.write_all(busy).await);
            }
            each(&idle);
        }
    })
}
