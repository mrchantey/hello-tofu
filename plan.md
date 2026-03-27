# hello tofu

We are building a terraform config generator in pure rust. Very early days. take ownership of the architecture, goal is a clean and beautiful typesafe terraform api.

## Binding Generator

The types are currently being generated with snake_case names, add an option that uses heck to convert them to `TitleCase`.

### Filtering

the `schema.json` emitted by tofu is incredibly large, we need an option to apply filtering, something like this:
the resulting full raw rust file would be incredibly large. we must use filtering:

ie the following

```rust
#[derive(Default)]
struct ResourceFilter{
	filters: HashMap<String,HashSet<String>>
}
impl ResourceFilters{
	pub fn with_resources(mut self, provider: impl Into<String>, resources:Vec<impl Into<String>>)->mut self{
		self.entry(provider).or_default().extend(resources);
	}
}
ResourceFilter::default()
	.with_resources("registry.opentofu.org/hashicorp/aws",vec![
		"aws_api_gateway_rest_api",
		"aws_lambda_function",
		"aws_s3_bucket"
	])
	.with_resources("registry.opentofu.org/cloudflare/cloudflare",vec![
		"cloudflare_dns_record"
	]);
```

Would result in only these objects being parsed into rust types.
```json
{
  "format_version": "1.0",
  "provider_schemas": {
    "registry.opentofu.org/hashicorp/aws": {
      "resource_schemas": {
        "aws_api_gateway_rest_api": {},
        "aws_lambda_function": {},
        "aws_s3_bucket": {}
      }
    },
    "registry.opentofu.org/cloudflare/cloudflare": {
      "resource_schemas": {
        "cloudflare_dns_record": {}
      }
    }
  }
}
```

### Own the project!
```rust
// Optionally strip the `Default` derive that the emitter always adds.
if !self.generate_default {
    code = code.replace(", Default", "");
}
```
stuff like this is rediculously lazy, we own the emitter so just shift options like that upstream to the emitter instead of doing it afterwards. Find similar patterns and clean up the codebase for this kind of ad-hoc messiness.
We're designing a production grade rust system here..

## Config Exporter rewrite

The whole point of this project is using actual rust types do delcare resources so we get type safety, not just using macros and json!().


This means rewriting the config exporter to be typed. It also means the BindingGenerator will need to implement these traits for all providers, resources etc types it generates.

```rust
/// this is just a back-of-napkin idea, but something like this will be required.

pub struct TerraProvider{
	name:String,
	version:String,
}

pub trait TerraResource:Value{
	fn name(&self)->str;
	// dunno, maaybe something like this?
	fn provider(&self)->&'static impl TerraProvider;
}

pub trait TerraJson{
	fn into_json(&self)->Value;
}

pub struct ConfigExporter {
    providers: HashSet<TerraProvider>,
    resources: Vec<Box<dyn TerraResource>>,
    ..
}

impl ConfigExporter{
	
	pub fn with_resource(mut self, resource: impl TerraResource)->self{
		..
	}
}

```

In the examples is a validation step, move this validation step to the exporter directly.

## Examples

Once thats done we can properly implement 
- `examples/lightsail.rs`
- `examples/lambda.rs`
removing all these random strings and json!, replacing with a vec of proper types.

Note the goal of these is to replace their typescript equivelents, they clearly dont yet, ie the lambda one makes no mention of the cloudflare dns provider..

These examples should be pretty small and concise, all the validation and boilerplate should be a part of the library itself.

The vision is for an elegant and typesafe tofu experience in rust, lets make it happen.
