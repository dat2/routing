error_chain! {
  errors {
    MissingAttribute(enum_variant: String) {
      description("missing attribute on enum variant")
      display("missing attribute on {}", enum_variant)
    }
  }
}
