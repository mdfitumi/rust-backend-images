use std::io::Write;

use actix_multipart::Multipart;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer};
use futures::{StreamExt, TryStreamExt};

async fn save_file(mut payload: Multipart) -> Result<HttpResponse, Error> {
    // iterate over multipart stream
    while let Ok(Some(mut field)) = payload.try_next().await {
        println!("field {:?}", field);
        let content_type = field.content_disposition().unwrap();
        println!("content_type {}", content_type);
        match content_type.get_filename() {
            Some(filename) => {
                let filepath = format!("./tmp/{}", sanitize_filename::sanitize(&filename));
                println!("{}", filepath);

                // File::create is blocking operation, use threadpool
                let mut f = web::block(|| std::fs::File::create(filepath))
                    .await
                    .unwrap();

                // Field in turn is stream of *Bytes* object
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    // filesystem operations are blocking, we have to use threadpool
                    f = web::block(move || f.write_all(&data).map(|_| f)).await?;
                }
            }
            None => {
                // accept text
                let text = field
                    .fold("".to_owned(), |acc, result| async {
                        match result {
                            Ok(val) => {
                                acc + std::str::from_utf8(&val)
                                    .expect("unable to read form value text")
                            }
                            Err(..) => acc,
                        }
                    })
                    .await;
                println!("text {}", text);
                let filepath = format!("./tmp/{}", sanitize_filename::sanitize(&content_type.get_name().expect("unable to read filename")));
                println!("{}", filepath);
                let mut f = web::block(|| std::fs::File::create(filepath))
                    .await
                    .unwrap();
                match base64::decode(&text) {
                    Ok(bytes) => web::block(move || f.write_all(&bytes).map(|_| ())).await?,
                    Err(..) => {
                        unimplemented!("handle url links is not supported yet");
                    }
                }
            }
        }
    }
    Ok(HttpResponse::Ok().into())
}

fn index() -> HttpResponse {
    let html = r#"<html>
        <head><title>Upload Test</title></head>
        <body>
            <form target="/" method="post" enctype="multipart/form-data">
                <input type="file" multiple name="file"/>
                <button type="submit">Submit</button>
            </form>
        </body>
    </html>"#;
    HttpResponse::Ok().content_type("text/html").body(html)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "actix_server=info,actix_web=info");
    std::fs::create_dir_all("./tmp").unwrap();

    HttpServer::new(|| {
        App::new().wrap(middleware::Logger::default()).service(
            web::resource("/")
                .route(web::get().to(index))
                .route(web::post().to(save_file)),
        )
    })
    .bind("0.0.0.0:3000")?
    .run()
    .await
}
