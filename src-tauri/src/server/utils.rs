
pub fn e500<T>(e: T) -> actix_web::Error
where
  T: std::fmt::Display + std::fmt::Debug + 'static,
{
  actix_web::error::ErrorInternalServerError(e)
}