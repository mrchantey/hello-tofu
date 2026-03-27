# hello tofu


We are building a terraform config generator in pure rust.


## Plan

### Binding generator

Malformed config files must be a rust compiletime error. to achive this we will generate rust bindings from tofu schema.

The terraform schema has been generated at `schema.json`.

I've added the `tfschema-bindgen` repository, this was last updated five years ago so likely needs some love.


Time to generate rust types from 