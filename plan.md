# hello tofu

We are building a terraform config generator in pure rust. Very early days. take ownership of the architecture, goal is a clean and beautiful typesafe terraform api.

## Binding Generator

The types are currently being generated with snake_case names, add an option that uses heck to convert them to `TitleCase`.


### Filtering

the `schema.json` emitted by tofu is incredibly large, we need an option to apply filtering, something like this:
the resulting full raw rust file would be incredibly large. we must use filtering:

ie the following

```rust
// something like this.. types will need some work

#[derive(Default)]
struct ResourceFilter{
	filters: HashMap<String,HashSet<String>>
}
impl ResourceFilters{
	pub fn with_resources(mut self, provider: impl Into<String>, resources:impl IntoIterator<impl Into<String>>)->mut self{
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

pub trait TerraResource: TerraJson{
	fn name(&self)->str;
	// dunno, maybe something like this, avoid unnessecary cloning
	fn provider(&self)->Cow<'static, TerraProvider>;
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
	/// providers needed is implied by the resources added..
	pub fn with_resource(mut self, resource: impl TerraResource)->self{
		..
	}
}

```

In the examples is a validation step, move this validation step to the exporter directly.


## ConfigGenerator

This is a roundtrip binding generator, exporting rust files to some path.

1. user specifies the providers, and their resources to generate
2. clear `target/terra-bindings-generator` and in there create a terraform config file.

```json
//providers.tf.json
{
	"terraform": {
		"required_providers": {
			"aws": {
				"source": "hashicorp/aws",
				"version": "~> 6.0"
			},
			"cloudflare": {
				"source": "cloudflare/cloudflare",
				"version": "~> 5.0"
			}
		}
	}
}
```
3. run `tofu init` in that directory..
4. run `tofu providers schema -json > schema.json`. this file will be very large so save to file.
5. parse the schema with the binding generator. 
6. export rust files to the specified directory path.

```rust
pub struct TerraProvider{
	/// Display name for the provider
	name: String,
	source: String,	
	version: String
}

impl TerraProvider{
	pub const Aws:Self = Self{
		name: "Amazon Web Services",
		source: "registry.opentofu.org/hashicorp/aws",
		version: "6.0"
	}
	pub const Cloudflare:Self = Self{
		name: "Cloudflare",
		source: "registry.opentofu.org/cloudflare/cloudflare",
		version: "5.0"
	}
}

pub struct TerraProviderGenerator{
	provider:TerraProvider,
	path: PathBuf
}


pub struct ConfigGenerator{
	resources :HashSet<TerraProviderGenerator, String>,
	binding_generator:BindingGenerator,
}
```

Then create an example that can start to populate our types.

```rust
//examples/generate.rs
// the exact resources here should be enough for our examples to work.

ConfigGenerator::default()
	.with_resources(TerraProviderGenerator::new(TerraPovider::Aws, "src/providers/aws_basics.rs"), vec![
		"aws_api_gateway_rest_api",
		"aws_lambda_function",
		"aws_s3_bucket"
	])
	.with_resources(TerraProviderGenerator::new(TerraPovider::Cloudflare, "src/providers/cloudflare_dns.rs"), vec![
		"cloudflare_dns_record"
	]);
```

Once that has run add features to this crate:
`providers_aws_basics`, `providers_cloudflare_dns` etc

ensure it compiles with these features..

## Examples

Once thats done we can properly implement 
- `examples/lightsail.rs`
- `examples/lambda.rs`
removing all these random strings and json!, replacing with a vec of proper types that are now in src/providers.

Note the goal of these is to replace their typescript equivelents, ie `examples/lambda.ts` they clearly dont yet, ie the lambda one makes no mention of the cloudflare dns provider..

These examples should be pretty small and concise, all the validation and boilerplate should be a part of the library itself.

The vision is for an elegant and typesafe tofu experience in rust, lets make it happen.
