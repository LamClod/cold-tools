use serde_json::{Map, Value, json};

/// Type-safe JSON Schema builder for tool parameter definitions.
pub struct Schema;

impl Schema {
    /// Start building an object schema.
    #[must_use]
    pub fn object() -> ObjectBuilder {
        ObjectBuilder {
            properties: Map::new(),
            required: Vec::new(),
            description: None,
        }
    }

    /// Create a string schema.
    #[must_use]
    pub const fn string() -> StringSchema {
        StringSchema {
            description: None,
            default: None,
            enum_values: None,
        }
    }

    /// Create an integer schema.
    #[must_use]
    pub const fn integer() -> IntegerSchema {
        IntegerSchema {
            description: None,
            default: None,
            minimum: None,
            maximum: None,
        }
    }

    /// Create a number schema.
    #[must_use]
    pub const fn number() -> NumberSchema {
        NumberSchema {
            description: None,
            default: None,
            minimum: None,
            maximum: None,
        }
    }

    /// Create a boolean schema.
    #[must_use]
    pub const fn boolean() -> BooleanSchema {
        BooleanSchema {
            description: None,
            default: None,
        }
    }

    /// Create an array schema with the given items schema.
    #[must_use]
    pub const fn array(items: Value) -> ArraySchema {
        ArraySchema {
            items,
            description: None,
        }
    }

    /// Create an enum schema from a list of string values.
    #[must_use]
    pub fn enum_values(values: &[&str]) -> Value {
        json!({
            "type": "string",
            "enum": values,
        })
    }
}

/// Builder for object schemas.
pub struct ObjectBuilder {
    properties: Map<String, Value>,
    required: Vec<String>,
    description: Option<String>,
}

impl ObjectBuilder {
    /// Add an optional property.
    #[must_use]
    pub fn property(mut self, name: &str, schema: impl Into<Value>) -> Self {
        self.properties.insert(name.to_string(), schema.into());
        self
    }

    /// Add a required property.
    #[must_use]
    pub fn required_property(mut self, name: &str, schema: impl Into<Value>) -> Self {
        self.properties.insert(name.to_string(), schema.into());
        self.required.push(name.to_string());
        self
    }

    /// Set the object description.
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    /// Build the JSON Schema value.
    #[must_use]
    pub fn build(self) -> Value {
        let mut obj = Map::new();
        obj.insert("type".to_string(), json!("object"));

        if let Some(desc) = self.description {
            obj.insert("description".to_string(), json!(desc));
        }

        if !self.properties.is_empty() {
            obj.insert("properties".to_string(), Value::Object(self.properties));
        }

        if !self.required.is_empty() {
            obj.insert("required".to_string(), json!(self.required));
        }

        Value::Object(obj)
    }
}

/// Schema for string types.
pub struct StringSchema {
    description: Option<String>,
    default: Option<String>,
    enum_values: Option<Vec<String>>,
}

impl StringSchema {
    /// Set the description.
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    /// Set the default value.
    #[must_use]
    pub fn default(mut self, val: &str) -> Self {
        self.default = Some(val.to_string());
        self
    }

    /// Set allowed enum values.
    #[must_use]
    pub fn enum_values(mut self, values: &[&str]) -> Self {
        self.enum_values = Some(values.iter().map(|s| (*s).to_string()).collect());
        self
    }
}

impl From<StringSchema> for Value {
    fn from(s: StringSchema) -> Self {
        let mut obj = Map::new();
        obj.insert("type".to_string(), json!("string"));
        if let Some(desc) = s.description {
            obj.insert("description".to_string(), json!(desc));
        }
        if let Some(def) = s.default {
            obj.insert("default".to_string(), json!(def));
        }
        if let Some(vals) = s.enum_values {
            obj.insert("enum".to_string(), json!(vals));
        }
        Self::Object(obj)
    }
}

/// Schema for integer types.
pub struct IntegerSchema {
    description: Option<String>,
    default: Option<i64>,
    minimum: Option<i64>,
    maximum: Option<i64>,
}

impl IntegerSchema {
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    #[must_use]
    pub const fn default(mut self, val: i64) -> Self {
        self.default = Some(val);
        self
    }

    #[must_use]
    pub const fn minimum(mut self, val: i64) -> Self {
        self.minimum = Some(val);
        self
    }

    #[must_use]
    pub const fn maximum(mut self, val: i64) -> Self {
        self.maximum = Some(val);
        self
    }
}

impl From<IntegerSchema> for Value {
    fn from(s: IntegerSchema) -> Self {
        let mut obj = Map::new();
        obj.insert("type".to_string(), json!("integer"));
        if let Some(desc) = s.description {
            obj.insert("description".to_string(), json!(desc));
        }
        if let Some(def) = s.default {
            obj.insert("default".to_string(), json!(def));
        }
        if let Some(min) = s.minimum {
            obj.insert("minimum".to_string(), json!(min));
        }
        if let Some(max) = s.maximum {
            obj.insert("maximum".to_string(), json!(max));
        }
        Self::Object(obj)
    }
}

/// Schema for number (float) types.
pub struct NumberSchema {
    description: Option<String>,
    default: Option<f64>,
    minimum: Option<f64>,
    maximum: Option<f64>,
}

impl NumberSchema {
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    #[must_use]
    pub const fn default(mut self, val: f64) -> Self {
        self.default = Some(val);
        self
    }

    #[must_use]
    pub const fn minimum(mut self, val: f64) -> Self {
        self.minimum = Some(val);
        self
    }

    #[must_use]
    pub const fn maximum(mut self, val: f64) -> Self {
        self.maximum = Some(val);
        self
    }
}

impl From<NumberSchema> for Value {
    fn from(s: NumberSchema) -> Self {
        let mut obj = Map::new();
        obj.insert("type".to_string(), json!("number"));
        if let Some(desc) = s.description {
            obj.insert("description".to_string(), json!(desc));
        }
        if let Some(def) = s.default {
            obj.insert("default".to_string(), json!(def));
        }
        if let Some(min) = s.minimum {
            obj.insert("minimum".to_string(), json!(min));
        }
        if let Some(max) = s.maximum {
            obj.insert("maximum".to_string(), json!(max));
        }
        Self::Object(obj)
    }
}

/// Schema for boolean types.
pub struct BooleanSchema {
    description: Option<String>,
    default: Option<bool>,
}

impl BooleanSchema {
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    #[must_use]
    pub const fn default(mut self, val: bool) -> Self {
        self.default = Some(val);
        self
    }
}

impl From<BooleanSchema> for Value {
    fn from(s: BooleanSchema) -> Self {
        let mut obj = Map::new();
        obj.insert("type".to_string(), json!("boolean"));
        if let Some(desc) = s.description {
            obj.insert("description".to_string(), json!(desc));
        }
        if let Some(def) = s.default {
            obj.insert("default".to_string(), json!(def));
        }
        Self::Object(obj)
    }
}

/// Schema for array types.
pub struct ArraySchema {
    items: Value,
    description: Option<String>,
}

impl ArraySchema {
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }
}

impl From<ArraySchema> for Value {
    fn from(s: ArraySchema) -> Self {
        let mut obj = Map::new();
        obj.insert("type".to_string(), json!("array"));
        obj.insert("items".to_string(), s.items);
        if let Some(desc) = s.description {
            obj.insert("description".to_string(), json!(desc));
        }
        Self::Object(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_schema() {
        let schema = Schema::object()
            .required_property("path", Schema::string().description("file path"))
            .property("limit", Schema::integer().default(100).minimum(1))
            .description("Read a file")
            .build();

        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["path"]["type"], "string");
        assert_eq!(schema["required"][0], "path");
    }

    #[test]
    fn test_enum_values() {
        let schema = Schema::enum_values(&["start", "stop", "list"]);
        assert_eq!(schema["enum"][0], "start");
    }
}
