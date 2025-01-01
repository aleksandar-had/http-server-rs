use std::net::TcpListener;
use std::str::Split;
use std::{
    borrow::Cow,
    collections::HashMap,
    env,
    fs::{self, File},
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    thread,
};

use flate2::write::GzEncoder;
use flate2::Compression;
use threadpool::ThreadPool;

const USER_AGENT: &str = "User-Agent";
const PATH: &str = "Path";
const SUPPORTED_ENCODING: &str = "gzip";
const N_WORKERS: usize = 5;

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();
    let pool = ThreadPool::new(N_WORKERS);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                pool.execute(move || {
                    println!(
                        "accepted new connection in thread {:?}",
                        thread::current().id()
                    );
                    handle_connection(stream);
                });
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}

fn parse_request(request_str: Cow<'_, str>) -> HashMap<String, String> {
    let mut details = HashMap::new();

    let mut split_request = request_str.split("\r\n");

    // Parse first line (Type Path Version)
    if let Some(str) = split_request.next() {
        let str_split: Vec<&str> = str.split(" ").collect();
        // println!("str_split: {:#?}", str_split);
        details.insert("Type".to_string(), str_split[0].to_string());
        details.insert("Path".to_string(), str_split[1].to_string());
        details.insert("Version".to_string(), str_split[2].to_string());
    }

    for data in split_request {
        let data_split: Vec<&str> = data.split(": ").collect();
        // println!("header_split: {:#?}", header_split);

        if data_split.len() == 2 {
            details.insert(data_split[0].to_string(), data_split[1].to_string());
        } else if data_split.len() == 1 {
            details.insert("Body".to_string(), data_split[0].to_string());
        }
    }

    println!("details hashmap: {:#?}", details);
    details
}

fn handle_connection(mut stream: TcpStream) {
    let mut buffer = [0; 1024];
    let bytes = stream.read(&mut buffer).unwrap();
    // IMPORTANT! Parse exactly as many bytes as have been read!
    let request_str = String::from_utf8_lossy(&buffer[..bytes]);

    let request_details = parse_request(request_str);

    println!("request details: {:#?}", request_details);

    let mut response: Vec<u8> = Vec::new();
    let path = request_details.get(PATH).unwrap();

    if path == "/" {
        response.extend_from_slice("HTTP/1.1 200 OK\r\n\r\n".as_bytes());
    } else if path.starts_with("/echo") {
        // encodings is a string with the following format: "{encoding1}, {encoding2}, {encoding3}, ..."
        let encodings = request_details
            .get("Accept-Encoding")
            .map_or("invalid", String::as_str);
        let echo = path.trim_start_matches("/echo/");
        response.extend_from_slice("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n".as_bytes());

        if contains_gzip_encoding(encodings.split(", ")) {
            response.extend_from_slice("Content-Encoding: gzip\r\n".as_bytes());

            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(echo.as_bytes()).unwrap();
            let compressed_data = encoder.finish().unwrap();
            // println!("compressed data: {:x?}", compressed_data);
            let content_len = format!("Content-Length: {}\r\n\r\n", compressed_data.len());
            response.extend_from_slice(content_len.as_bytes());
            response.extend_from_slice(&compressed_data);
        } else {
            response.extend_from_slice(
                format!("Content-Length: {}\r\n\r\n{}", echo.len(), echo).as_bytes(),
            );
        }
    } else if path.starts_with("/files") {
        let args: Vec<String> = env::args().collect();
        let dir = &args[2];
        let file_name = path.trim_start_matches("/files/");
        let file_path = Path::new(dir).join(file_name);

        response.extend_from_slice(parse_files_endpoint(&request_details, &file_path).as_bytes());
    } else if path == "/user-agent" {
        if let Some(user_agent) = request_details.get(USER_AGENT) {
            response.extend_from_slice(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    user_agent.len(),
                    user_agent
                )
                .as_bytes(),
            );
        }
    } else {
        response.extend_from_slice("HTTP/1.1 404 Not Found\r\n\r\n".as_bytes());
    }

    println!("response: {:?}", &response);

    stream.write_all(&response).unwrap();
}

fn parse_files_endpoint(request_details: &HashMap<String, String>, file_path: &PathBuf) -> String {
    let response: String;
    let request_type = request_details.get("Type").unwrap().as_str();

    match request_type {
        "GET" => {
            let file_reader = fs::read_to_string(file_path);
            match file_reader {
                Ok(file_contents) => {
                    response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\n\r\n{}",
                            file_contents.len(),
                            file_contents
                        );
                }
                Err(_) => {
                    response = format!("HTTP/1.1 404 Not Found\r\n\r\n");
                }
            }
        }
        "POST" => {
            let mut file = File::create(file_path).unwrap();
            let file_content = request_details.get("Body").unwrap();
            file.write_all(file_content.as_bytes()).unwrap();
            response = format!("HTTP/1.1 201 Created\r\n\r\n");
        }
        _ => {
            response = format!("HTTP/1.1 404 Not Found\r\n\r\n");
        }
    }

    return response;
}

fn contains_gzip_encoding(encodings: Split<&str>) -> bool {
    for encoding in encodings {
        if encoding == SUPPORTED_ENCODING {
            return true;
        }
    }

    false
}
