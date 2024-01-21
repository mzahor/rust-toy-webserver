use std::{
    fs,
    io::{BufRead, BufReader, ErrorKind, Write},
    marker::Send,
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

struct Worker {
    thread: Option<JoinHandle<()>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl Worker {
    pub fn new(id: u32, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        Worker {
            thread: Some(thread::spawn(move || loop {
                let f = receiver.lock().unwrap().recv();
                match f {
                    Ok(f) => {
                        println!("Got a new task worker_id:{id}");
                        f();
                    }
                    Err(_) => {
                        println!("Worker {id} disconnected, exiting...");
                        break;
                    }
                }
            })),
        }
    }
}

struct ThreadPool {
    tx: Option<mpsc::Sender<Job>>,
    workers: Vec<Worker>,
}

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        assert!(size > 0, "size <= 0");
        let (tx, rx) = mpsc::channel();
        let rx = Arc::new(Mutex::new(rx));
        let mut workers = Vec::with_capacity(size);
        for i in 0..size {
            let id: u32 = (i + 1).try_into().unwrap();
            workers.push(Worker::new(id, rx.clone()))
        }

        ThreadPool {
            tx: Some(tx),
            workers,
        }
    }

    fn run<T>(&mut self, job: T) -> Result<(), &str>
    where
        T: FnOnce() + Send + 'static,
    {
        if let Some(tx) = &self.tx {
            tx.send(Box::new(job)).unwrap();
            return Ok(());
        } else {
            return Err("sender already dropped");
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        let tx = self.tx.take();
        drop(tx);

        for worker in &mut self.workers {
            if let Some(thread) = worker.thread.take() {
                thread.join().unwrap();
            }
        }
    }
}

fn main() {
    let tcp_listener = TcpListener::bind("0.0.0.0:7878").unwrap();
    println!("Server started");
    let mut pool = ThreadPool::new(10);
    let should_exit = Arc::new(AtomicBool::new(false));

    {
        let should_exit = should_exit.clone();
        ctrlc::set_handler(move || should_exit.store(true, Ordering::SeqCst))
            .expect("Failed to set Ctrl+C handler");
    }

    tcp_listener
        .set_nonblocking(true)
        .expect("Failed to set non-blocking");

    for stream in tcp_listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("Incoming connection accepted");
                pool.run(move || handle_connection(stream)).unwrap();
            }
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                if should_exit.load(Ordering::SeqCst) {
                    println!("Ctrl+C pressed, exiting...");
                    break;
                }
                thread::yield_now();
                // loop
            }
            Err(err) => {
                println!("Failed connection {err}");
            }
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    let reader = BufReader::new(&mut stream);
    match reader.lines().next().unwrap() {
        Ok(request) => {
            let (file, status) = match &request[..] {
                "GET / HTTP/1.1" => ("index.html", "200 OK"),
                "GET /sleep HTTP/1.1" => {
                    thread::sleep(Duration::from_secs(5));
                    ("sleep.html", "200 OK")
                }
                _ => ("404.html", "404 NOT FOUND"),
            };
            response_file(&mut stream, file, status);
        }
        Err(err) => {
            println!("Handle connection error: {err}");
        }
    }
}

fn response_file(stream: &mut TcpStream, fname: &str, status: &str) {
    let html = fs::read_to_string(fname).unwrap();
    let status_line = format!("HTTP/1.1 {status}");
    let content_len_header = format!("Content-Length: {}", html.len());
    let response = format!("{status_line}\r\n{content_len_header}\r\n\r\n{html}\r\n\r\n");
    stream.write_all(response.as_bytes()).unwrap();
}
