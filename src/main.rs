use std::io::Write;

use actix_multipart::Multipart;
use actix_web::{middleware, web, App, Error, HttpResponse, HttpServer};
use futures::{StreamExt, TryStreamExt};

async fn create_and_save_preview(
    buffer: &[u8],
    filename: &str,
) -> std::result::Result<(), image::ImageError> {
    let image = image::load_from_memory(buffer)?;
    let filename = format!("./tmp/preview_{}", filename);

    web::block(move || {
        image
            .resize(100, 100, image::imageops::FilterType::Triangle)
            .save(filename)
    })
    .await
    .expect("unable to save image preview");
    Ok(())
}

async fn save_file(mut payload: Multipart) -> Result<HttpResponse, Error> {
    // iterate over multipart stream
    while let Ok(Some(field)) = payload.try_next().await {
        let content_type = field
            .content_disposition()
            .expect("invalid multipart content");
        match content_type.get_filename() {
            Some(filename) => {
                let filepath = format!("./tmp/{}", sanitize_filename::sanitize(&filename));

                // File::create is blocking operation, use threadpool
                let mut f = web::block(|| std::fs::File::create(filepath))
                    .await
                    .expect("unable to create image file");

                let multipart_data = field
                    .fold(Vec::new(), |mut acc, result| async {
                        match result {
                            Ok(val) => {
                                acc.extend(val);
                                acc
                            }
                            Err(..) => acc,
                        }
                    })
                    .await;
                let multipart_data_file = multipart_data.clone();
                web::block(move || f.write_all(&multipart_data_file)).await?;

                create_and_save_preview(&multipart_data, filename.clone())
                    .await
                    .map_err(|_| HttpResponse::BadRequest().body("cannot make image preview"))?;
            }
            None => {
                // accept base64 images or url links
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
                let filename = sanitize_filename::sanitize(
                    &content_type.get_name().expect("unable to read filename"),
                );
                let filepath = format!("./tmp/{}", filename);
                let mut f = web::block(|| std::fs::File::create(filepath))
                    .await
                    .unwrap();
                let bytes = match base64::decode(&text) {
                    Ok(bytes) => bytes,
                    Err(..) => reqwest::get(&text)
                        .await
                        .map_err(|_| {
                            HttpResponse::BadRequest().body("unable to get image from url")
                        })?
                        .bytes()
                        .await
                        .map_err(|_| HttpResponse::BadRequest().body("invalid image response"))
                        .map(|bytes| bytes.to_vec())?,
                };
                let image_bytes = bytes.clone();
                web::block(move || f.write_all(&image_bytes).map(|_| f)).await?;

                create_and_save_preview(&bytes, &filename)
                    .await
                    .map_err(|_| HttpResponse::BadRequest().body("cannot make image preview"))?;
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
