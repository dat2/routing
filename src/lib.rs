extern crate hyper;

pub trait RoutingTable<T> {
  fn route(request: &hyper::Request) -> Option<T>;
}
