extern crate hyper;

pub trait NewRoutingTable<T> {
  type Table: RoutingTable<T>;
  fn routing_table() -> Self::Table;
}

pub trait RoutingTable<T> {
  fn route(&self, request: &hyper::Request) -> Option<T>;
}
