use std::{
    collections::VecDeque,
    fs,
    io::{BufRead, BufReader, Write},
    marker::Send,
    net::{TcpListener, TcpStream},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};

struct Worker {
    id: u32,
    handle: JoinHandle<()>,
    receiver: Arc<Mutex<mpsc::Receiver<Job>>>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl Worker {
    pub fn new(id: u32, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Worker {
        Worker {
            id,
            receiver: receiver.clone(),
            handle: thread::spawn(move || loop {
                let f = receiver.lock().unwrap().recv().unwrap();
                println!("Got a new task worker_id:{id}");
                f();
            }),
        }
    }
}

struct ThreadPool {
    size: usize,
    tx: mpsc::Sender<Job>,
    queue: VecDeque<Job>,
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
            size,
            tx,
            queue: VecDeque::new(),
            workers,
        }
    }

    fn run<T>(&mut self, job: T)
    where
        T: FnOnce() + Send + 'static,
    {
        self.tx.send(Box::new(job)).unwrap();
    }
}

fn main() {
    let tcp_listener = TcpListener::bind("0.0.0.0:7878").unwrap();
    println!("Server started");
    let mut pool = ThreadPool::new(10);
    for stream in tcp_listener.incoming() {
        let stream = stream.unwrap();
        println!("Incoming connection accepted");
        pool.run(move || handle_connection(stream));
    }
}

fn handle_connection(mut stream: TcpStream) {
    let reader = BufReader::new(&mut stream);
    let request = reader.lines().next().unwrap().unwrap();
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

fn response_file(stream: &mut TcpStream, fname: &str, status: &str) {
    let html = fs::read_to_string(fname).unwrap();
    let status_line = format!("HTTP/1.1 {status}");
    let content_len_header = format!("Content-Length: {}", html.len());
    let response = format!("{status_line}\r\n{content_len_header}\r\n\r\n{html}\r\n\r\n");
    stream.write_all(response.as_bytes()).unwrap();
}
