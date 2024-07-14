use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use warp::Filter;
use serde_json::json;

const RSYNCD_VERSION_PREFIX: &str = "@RSYNCD:";
const RSYNCD_SERVER_VERSION: &str = "@RSYNCD: 31.0\n";
const RSYNCD_EXIT: &str = "@RSYNCD: EXIT\n";

const MOTD: &str = "Welcome to sdu rsync server\n";


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let module_list: Arc<Mutex<HashMap<String, Vec<String>>>> = Arc::new(Mutex::new(HashMap::new()));
    {
        let mut module_list = module_list.lock().unwrap();
        module_list.insert("termux".to_string(), vec!["101.6.15.130:873".to_string()]);
    }

    // 监听本地地址和端口
    let listener = TcpListener::bind("0.0.0.0:873").await?;

    println!("Server running on 0.0.0.0:873");

    {
        let module_list_clone1 = Arc::clone(&module_list);
        let module_list_clone2 = Arc::clone(&module_list);

        tokio::spawn(async move {
            // /config 返回配置信息
            let routes = warp::path!("config")
                .map(move || {
                    let module_list = module_list_clone1.lock().unwrap();
                    // 返回json格式的数据（使用warp的json)
                    /*
                    [
                        {
                            "module_name": "termux",
                            "upstreams": [
                                "xxx.xxx.xxx.xxx:***"
                            ],
                        }
                    ]
                     */

                    let mut result = Vec::new();
                    for (k, v) in module_list.iter() {
                        let mut upstreams = Vec::new();
                        for upstream in v.iter() {
                            upstreams.push(upstream.as_str());
                        }
                        let module = json!({
                            "module_name": k,
                            "upstreams": upstreams,
                        });
                        result.push(module);
                    }

                    warp::reply::json(&result)
                });

            // /update 更新配置信息(格式与/config一致)

            let routes = routes.or(warp::path!("update")
                .and(warp::body::json())
                .map(move |data: Vec<serde_json::Value>| {
                    let mut module_list = module_list_clone2.lock().unwrap();
                    module_list.clear();
                    for module in data.iter() {
                        let module_name = module["module_name"].as_str().unwrap();
                        let mut upstreams = Vec::new();
                        for upstream in module["upstreams"].as_array().unwrap() {
                            upstreams.push(upstream.as_str().unwrap().to_string());
                        }
                        module_list.insert(module_name.to_string(), upstreams);
                    }
                    warp::reply::json(&data)
                }));


            warp::serve(routes)
                .run(([127, 0, 0, 1], 3030))
                .await;

        });
    }

    loop {
        // 异步等待客户端连接
        let (mut socket, _) = listener.accept().await?;

        let module_list_clone = Arc::clone(&module_list);

        tokio::spawn(async move {
            let mut buf = vec![0; 1024];
            let mut ret = 0;

            let ip_addr = socket.peer_addr().unwrap().ip();
            println!("Client connected from: {}", ip_addr);

            let mut rsync_version = String::new();

            // 读取客户端发送的数据
            ret = socket.read(&mut buf).await.unwrap();
            rsync_version.push_str(std::str::from_utf8(&buf[..ret]).unwrap());

            if !rsync_version.starts_with(RSYNCD_VERSION_PREFIX) {
                println!("Invalid rsync version: {}", rsync_version);
                return;
            }

            println!("Client rsync version: {}", rsync_version);

            socket.write_all(RSYNCD_SERVER_VERSION.as_bytes()).await.unwrap();

            let mut module_name = String::new();
            ret = socket.read(&mut buf).await.unwrap();
            module_name.push_str(std::str::from_utf8(&buf[..ret]).unwrap());

            println!("Client module name: {}", module_name);

            if !module_name.ends_with("\n") {
                println!("Invalid module name: {}", module_name);
                return;
            }

            if !MOTD.is_empty() {
                socket.write_all(MOTD.as_bytes()).await.unwrap();
            }

            if module_name == "\n" {
                let mut msg = String::new();
                let module_list;
                {
                    module_list = module_list_clone.lock().unwrap().clone();
                    for (k, _) in module_list.iter() {
                        msg.push_str(k.as_str());
                        msg.push_str("\n");
                    }
                }
                msg.push_str(RSYNCD_EXIT);
                socket.write_all(msg.as_bytes()).await.unwrap();
                return;
            }

            module_name.pop();


            if !module_list_clone.lock().unwrap().contains_key(module_name.as_str()) {
                // 归还锁
                let msg = format!("unknown module: {}\n", module_name);
                socket.write(msg.as_bytes()).await.unwrap();
                socket.write_all(RSYNCD_EXIT.as_bytes()).await.unwrap();
                return;
            }

            let upstreams = module_list_clone.lock().unwrap().get(module_name.as_str()).unwrap().clone();

            //TODO : random select upstream
            let upstream = &upstreams[0];

            let mut upstream_socket = tokio::net::TcpStream::connect(upstream).await.unwrap();

            upstream_socket.write_all(rsync_version.as_bytes()).await.unwrap();
            let mut upstream_version = String::new();
            ret = upstream_socket.read(&mut buf).await.unwrap();
            upstream_version.push_str(std::str::from_utf8(&buf[..ret]).unwrap());

            if !upstream_version.starts_with(RSYNCD_VERSION_PREFIX) {
                println!("Invalid upstream rsync version: {}", upstream_version);
                return;
            }

            println!("Upstream rsync version: {}", upstream_version);

            let idx = upstream_version.find("\n");

            if !idx.is_none() {
                let upstream_motd = &upstream_version[idx.unwrap()..];
                socket.write_all(upstream_motd.as_bytes()).await.unwrap();
            }

            upstream_socket.write_all((module_name + "\n").as_bytes()).await.unwrap();

            let (mut socket_reader, mut socket_writer) = socket.split();
            let (mut upstream_reader, mut upstream_writer) = upstream_socket.split();

            tokio::select! {
                _ = tokio::io::copy(&mut socket_reader, &mut upstream_writer) => {},
                _ = tokio::io::copy(&mut upstream_reader, &mut socket_writer) => {},
            }
        });
    }
}
