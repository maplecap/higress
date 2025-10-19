use higress_wasm_rust::cluster_wrapper::DnsCluster;
use higress_wasm_rust::log::Log;
use higress_wasm_rust::plugin_wrapper::{HttpContextWrapper, RootContextWrapper};
use higress_wasm_rust::request_wrapper;
use higress_wasm_rust::rule_matcher::{on_configure, RuleMatcher, SharedRuleMatcher};
use http::Method;
use multimap::MultiMap;
use proxy_wasm::traits::{Context, HttpContext, RootContext};
use proxy_wasm::types::{Bytes, ContextType, DataAction, HeaderAction, LogLevel};
use serde::de::Deserializer;
use serde::{de, Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Display;
use std::ops::DerefMut;
use std::rc::{Rc, Weak};
use std::string::ToString;
use std::time::Duration;

proxy_wasm::main! {{
    proxy_wasm::set_log_level(LogLevel::Debug);
    proxy_wasm::set_root_context(|_|Box::new(TokenPreludeRoot::new()));
}}

const PLUGIN_NAME: &str = "ai-token-prelude";
const PROVIDER_HEADER: &str = "x-ks-provider";
const _PROVIDER_PUBLIC: &str = "public";
const PROVIDER_INTERNAL: &str = "internal";
const WQ_USE_MODEL_HEADER: &str = "x-wq-use-model-type";
const WQ_USE_MODEL_INTERNAL: &str = "internal";
const MODEL_HEADER: &str = "x-higress-llm-model";
const PRIVATE_MODEL_KEY: &str = "private";
const TOKEN_PRELUDE_HEADER: &str = "x-ks-token-prelude";
const NEED_TOKEN_PRELUDE_CTX: &str = "need-token-prelude";
const DEFAULT_TIME_OUT: u64 = 2 * 60 * 1000;
const _DEFAULT_MAX_BODY_BYTES: u32 = 100 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PilotConfig {
    #[serde(default = "default_pilot_name")]
    service_name: String,
    #[serde(default = "default_pilot_port")]
    service_port: Option<u16>,
    #[serde(default = "default_pilot_domain")]
    service_domain: String,
    #[serde(default = "default_pilot_path")]
    service_path: String,
}

impl Default for PilotConfig {
    fn default() -> Self {
        Self {
            service_name: String::new(),
            service_port: None,
            service_domain: String::new(),
            service_path: String::new(),
        }
    }
}

fn default_pilot_name() -> String {
    "ai-token-pilot".to_string()
}

fn default_pilot_port() -> Option<u16> {
    Some(8000)
}

fn default_pilot_domain() -> String {
    "ai-token-pilot.higress-system.svc".to_string()
}

fn default_pilot_path() -> String {
    "/healthz".to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
enum RuleValue {
    Str(String),
    Set(HashMap<String, ()>),
}

impl Default for RuleValue {
    fn default() -> Self {
        RuleValue::Str(String::new())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
enum Operator {
    In,
    NotIn,
    Equal,
    NotEqual,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    NotContains,
    Regexp,
}

impl Default for Operator {
    fn default() -> Self {
        Operator::In
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
struct RuleItem {
    key: String,
    operator: Operator,
    #[serde(deserialize_with = "deserialize_rule_value")]
    value: RuleValue,
}

fn deserialize_rule_value<'de, D>(deserializer: D) -> Result<RuleValue, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::String(s) => Ok(RuleValue::Str(s)),
        Value::Array(arr) => {
            let mut set = HashMap::new();
            for item in arr {
                if let Value::String(s) = item {
                    set.insert(s, ());
                } else {
                    return Err(de::Error::custom("rule_value expected string in array"));
                }
            }
            Ok(RuleValue::Set(set))
        }
        _ => Err(de::Error::custom("rule_value unexpected value type")),
    }
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
struct ComplexRule {
    rule_items: Vec<RuleItem>,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
struct Header {
    key: String,
    value: String,
}

#[derive(Default, Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
struct TokenPreludeConfig {
    active: bool,
    #[serde(default = "default_version")]
    version: String,
    need_prelude_headers: Vec<Header>,
    forbid_prelude_headers: Vec<Header>,
    // support_models: Option<Vec<String>>,
    // support_models_map: HashMap<String, ()>,
    #[serde(deserialize_with = "deserialize_support_models")]
    support_models: HashMap<String, ()>,
    token_high_threshold: HashMap<String, f64>,
    token_low_threshold: HashMap<String, f64>,
    token_prelude_coefficient: HashMap<String, f64>,
    whitelist_prelude_headers: Vec<Header>,
    pilot_service: PilotConfig,
    complex_rules: Vec<ComplexRule>,
}

fn deserialize_support_models<'de, D>(deserializer: D) -> Result<HashMap<String, ()>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(deserializer)?;
    match v {
        Value::Array(arr) => {
            let mut set = HashMap::new();
            for item in arr {
                if let Value::String(s) = item {
                    set.insert(s, ());
                } else {
                    return Err(de::Error::custom("support_models expected string in array"));
                }
            }
            Ok(set)
        }
        _ => Err(de::Error::custom("support_models unexpected value type")),
    }
}

fn default_version() -> String {
    "v1".to_string()
}

impl Display for TokenPreludeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match serde_json::to_string_pretty(self) {
            Ok(json) => write!(f, "{}", json),
            Err(_) => write!(f, "failed to serialize TokenPreludeConfig"),
        }
    }
}

// impl TokenPreludeConfig {
//     fn support_models_to_map(&self) -> HashMap<String, ()> {
//         self.support_models
//             .as_ref()
//             .unwrap_or(&Vec::new())
//             .iter()
//             .map(|m| (m.clone(), ()))
//             .collect()
//     }
// }

fn format_body(body: Option<Vec<u8>>) -> String {
    if let Some(bd) = &body {
        if let Ok(b) = std::str::from_utf8(bd) {
            return b.to_string();
        }
    }
    format!("{:?}", body)
}

struct TokenPrelude {
    // 每个请求对应的插件实例
    log: Log,
    config: Option<Rc<TokenPreludeConfig>>,
    weak: Weak<RefCell<Box<dyn HttpContextWrapper<TokenPreludeConfig>>>>,
    user_context: HashMap<String, Box<dyn Any>>,
    cid: i64,
}

impl Context for TokenPrelude {}
impl HttpContext for TokenPrelude {}
impl HttpContextWrapper<TokenPreludeConfig> for TokenPrelude {
    fn init_self_weak(
        &mut self,
        self_weak: Weak<RefCell<Box<dyn HttpContextWrapper<TokenPreludeConfig>>>>,
    ) {
        self.weak = self_weak;
        self.log.info("init_self_rc");
    }
    fn log(&self) -> &Log {
        &self.log
    }
    fn on_config(&mut self, config: Rc<TokenPreludeConfig>) {
        self.log.info(&format!("on_config: {}", config));
        self.config = Some(config.clone());
    }
    fn on_http_request_complete_headers(
        &mut self,
        headers: &MultiMap<String, String>,
    ) -> HeaderAction {
        self.log
            .debug(&format!("on_http_request_complete_headers {:?}", headers));

        let Some(config) = self.config.clone() else {
            self.log.error("failed to find token-prelude config");
            return HeaderAction::Continue;
        };

        if !config.active {
            self.log
                .debug(&format!("token-prelude active: {}", config.active));
            return HeaderAction::Continue;
        }

        if !self.need_prelude(&config.clone(), headers) {
            return HeaderAction::Continue;
        }

        if self.forbid_prelude(&config.clone(), headers) {
            return HeaderAction::Continue;
        }

        if self.in_prelude_whitelist(&config.clone(), headers) {
            return HeaderAction::Continue;
        }

        if !request_wrapper::has_request_body() {
            return HeaderAction::Continue;
        }

        self.set_context(NEED_TOKEN_PRELUDE_CTX, Box::new(true));

        HeaderAction::StopIteration
    }
    fn on_http_response_complete_headers(
        &mut self,
        headers: &MultiMap<String, String>,
    ) -> HeaderAction {
        self.log
            .debug(&format!("on_http_response_complete_headers {:?}", headers));

        let _self_rc = match self.weak.upgrade() {
            Some(rc) => rc.clone(),
            None => {
                self.log.error("self_weak upgrade error");
                return HeaderAction::Continue;
            }
        };

        let Some(config) = self.config.clone() else {
            self.log.error("failed to find token-prelude config");
            return HeaderAction::Continue;
        };

        if !config.active {
            return HeaderAction::Continue;
        }

        self.set_http_response_header(
            // self_rc.borrow_mut().set_http_response_header( // panic
            TOKEN_PRELUDE_HEADER,
            Some(if self.get_string_context(TOKEN_PRELUDE_HEADER) == "" {
                "0"
            } else {
                self.get_string_context(TOKEN_PRELUDE_HEADER)
            }),
        );

        HeaderAction::Continue
    }
    fn cache_request_body(&self) -> bool {
        true
    }
    fn cache_response_body(&self) -> bool {
        false
    }
    fn on_http_request_complete_body(&mut self, req_body: &Bytes) -> DataAction {
        self.log.debug(&format!(
            "on_http_request_complete_body {}",
            String::from_utf8(req_body.clone()).unwrap_or("".to_string())
        ));

        let _self_rc = match self.weak.upgrade() {
            Some(rc) => rc.clone(),
            None => {
                self.log.error("self_weak upgrade error");
                return DataAction::Continue;
            }
        };

        let Some(config) = self.config.clone() else {
            self.log.error("failed to find token-prelude config");
            return DataAction::Continue;
        };

        if !config.active {
            return DataAction::Continue;
        }

        if !self.get_bool_context(NEED_TOKEN_PRELUDE_CTX) {
            return DataAction::Continue;
        }

        let model = {
            let v: serde_json::Value =
                serde_json::from_slice(req_body).unwrap_or(serde_json::Value::Null);
            v.get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string()
        };
        self.log.debug(&format!("xxx model: {}", &model));

        if !self.support_prelude(&config, &model) {
            self.log
                .debug(&format!("xxx model: {} has no external model", model));
            return DataAction::Continue;
        }

        let coefficient = config
            .token_prelude_coefficient
            .get(&model)
            .cloned()
            .unwrap_or_else(|| {
                *config
                    .token_prelude_coefficient
                    .get("default")
                    .unwrap_or(&0.33)
            });
        self.log.debug(&format!("xxx coefficient: {}", coefficient));

        let mut token_high_threshold = config
            .token_high_threshold
            .get(&model)
            .cloned()
            .unwrap_or_else(|| {
                *config
                    .token_high_threshold
                    .get("default")
                    .unwrap_or(&16_000f64)
            });
        if token_high_threshold <= 0f64 {
            token_high_threshold = 16_000f64;
        }
        self.log
            .debug(&format!("xxx tokenHighThreshold: {}", token_high_threshold));

        let mut token_low_threshold = config
            .token_low_threshold
            .get(&model)
            .cloned()
            .unwrap_or_else(|| {
                *config
                    .token_low_threshold
                    .get("default")
                    .unwrap_or(&14_000f64)
            });
        if token_low_threshold <= 0f64 {
            token_low_threshold = 14_000f64;
        }
        self.log
            .debug(&format!("xxx tokenLowThreshold: {}", token_low_threshold));

        let fuzzy_token = self.calculate_body_token(req_body, coefficient);
        self.log.debug(&format!("xxx fuzzyToken: {}", fuzzy_token));

        let mut original_headers: HashMap<String, String> =
            HashMap::from_iter(self.get_http_request_headers());

        original_headers.insert(TOKEN_PRELUDE_HEADER.to_string(), fuzzy_token.to_string());
        self.set_http_request_headers(
            original_headers
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect(),
        );
        self.set_context(TOKEN_PRELUDE_HEADER, Box::new(fuzzy_token.to_string()));

        if fuzzy_token >= token_high_threshold || fuzzy_token <= token_low_threshold {
            return DataAction::Continue;
        }

        self.calculate_body_token_precisely(req_body, &config)
    }
}

impl TokenPrelude {
    fn set_context(&mut self, key: &str, value: Box<dyn Any>) {
        self.user_context.insert(key.to_string(), value);
    }

    fn get_bool_context(&self, key: &str) -> bool {
        let Some(value) = self.user_context.get(key) else {
            return false;
        };
        value.downcast_ref::<bool>().copied().unwrap_or(false)
    }

    fn get_string_context(&self, key: &str) -> &str {
        let Some(value) = self.user_context.get(key) else {
            return "";
        };

        let Some(s) = value.downcast_ref::<String>() else {
            return "";
        };

        s.as_str()
    }

    fn calculate_body_token(&mut self, req_body: &Bytes, coefficient: f64) -> f64 {
        let mut text_str = String::new();

        let v: Value = match serde_json::from_slice(req_body) {
            Ok(val) => val,
            Err(_) => return 0f64,
        };

        if let Some(messages) = v.get("messages").and_then(|m| m.as_array()) {
            for msg in messages {
                // if let Some(contents) = msg.get("content").and_then(|c| c.as_array()) {
                //     for item in contents {
                //         match item.get("type") {
                //             None => {
                //                 if let Some(s) = item.as_str() {
                //                     text_str.push_str(s);
                //                 }
                //             }
                //             Some(t) if t == "text" => {
                //                 if let Some(text) = item.get("text").and_then(|s| s.as_str()) {
                //                     text_str.push_str(text);
                //                 }
                //             }
                //             _ => {}
                //         }
                //     }
                // }
                if let Some(content) = msg.get("content") {
                    match content {
                        Value::String(content) => {
                            text_str.push_str(content.as_str());
                        }
                        Value::Array(arr) => {
                            for item in arr {
                                match item.get("type") {
                                    Some(t) if t == "text" => {
                                        if let Some(text) =
                                            item.get("text").and_then(|s| s.as_str())
                                        {
                                            text_str.push_str(text);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        text_str.into_bytes().len() as f64 * coefficient
    }

    fn calculate_body_token_precisely(
        &mut self,
        req_body: &Bytes,
        config: &TokenPreludeConfig,
    ) -> DataAction {
        let self_rc = match self.weak.upgrade() {
            Some(rc) => rc.clone(),
            None => {
                self.log.error("self_weak upgrade error");
                return DataAction::Continue;
            }
        };

        let headers = [("Content-Type".to_string(), "application/json".to_string())];

        let cluster = DnsCluster::new(
            &config.pilot_service.service_name,
            &config.pilot_service.service_domain,
            config.pilot_service.service_port.unwrap(),
        );

        let http_call_res = self.http_call(
            &cluster,
            &Method::POST,
            &config.pilot_service.service_path,
            MultiMap::from_iter(headers),
            Some(req_body),
            Box::new(move |status_code, _response_headers, response_body| {
                if let Some(this) = self_rc.borrow_mut().downcast_mut::<TokenPrelude>() {
                    this.log
                        .debug("[calculate_body_token_precisely] callback called...");

                    let Ok(resp) =
                        serde_json::from_slice::<Value>(&response_body.unwrap_or(Vec::new()))
                    else {
                        this.log
                            .error("[calculate_body_token_precisely] parse resp_body failed");
                        this.resume_http_response();
                        return;
                    };

                    if status_code != 200 {
                        this.log.error(&format!(
                            "[calculate_body_token_precisely] response status code is {}, not 200",
                            status_code
                        ));
                        this.resume_http_response();
                        return;
                    }

                    let precise_token =
                        resp.get("data")
                            .and_then(|d| d.as_f64())
                            .unwrap_or_else(|| {
                                this.log.error(
                                "[calculate_body_token_precisely] missing or invalid data field",
                            );
                                0f64
                            });

                    let mut original_headers: HashMap<String, String> =
                        HashMap::from_iter(this.get_http_request_headers());
                    original_headers
                        .insert(TOKEN_PRELUDE_HEADER.to_string(), precise_token.to_string());
                    this.set_http_request_headers(
                        original_headers
                            .iter()
                            .map(|(k, v)| (k.as_str(), v.as_str()))
                            .collect(),
                    );

                    this.set_context(TOKEN_PRELUDE_HEADER, Box::new(precise_token.to_string()));

                    this.resume_http_request();
                } else {
                    self_rc.borrow().resume_http_response();
                }
            }),
            Duration::from_secs(DEFAULT_TIME_OUT),
        );
        match http_call_res {
            Ok(_) => return DataAction::StopIterationAndBuffer,
            Err(e) => {
                self.log.error(&format!("http_call fail {:?}", e));
            }
        }
        DataAction::Continue
    }

    fn need_prelude(
        &mut self,
        config: &TokenPreludeConfig,
        headers: &MultiMap<String, String>,
    ) -> bool {
        if config.need_prelude_headers.len() == 0 {
            return self.is_internal_request(config, headers);
        }

        config
            .need_prelude_headers
            .iter()
            .filter(|&h| match headers.get(&h.key) {
                None => false,
                Some(val) => val.eq(&h.value),
            })
            .collect::<Vec<&Header>>()
            .len()
            > 0
    }

    fn forbid_prelude(
        &mut self,
        config: &TokenPreludeConfig,
        headers: &MultiMap<String, String>,
    ) -> bool {
        if config.forbid_prelude_headers.len() == 0 {
            return self.only_internal_model(config, headers);
        }

        config
            .forbid_prelude_headers
            .iter()
            .filter(|&h| match headers.get(&h.key) {
                None => false,
                Some(val) => val.eq(&h.value),
            })
            .collect::<Vec<&Header>>()
            .len()
            > 0
    }

    fn support_prelude(&mut self, config: &TokenPreludeConfig, model: &str) -> bool {
        // config.support_models_map.contains_key(model)
        // config.support_models_to_map().contains_key(model)
        config.support_models.contains_key(model)
    }

    fn in_prelude_whitelist(
        &mut self,
        config: &TokenPreludeConfig,
        headers: &MultiMap<String, String>,
    ) -> bool {
        if config.whitelist_prelude_headers.len() == 0 {
            return self.is_private_request(config, headers);
        }

        config
            .whitelist_prelude_headers
            .iter()
            .filter(|&h| match headers.get(&h.key) {
                None => false,
                Some(val) => val.contains(&h.value),
            })
            .collect::<Vec<&Header>>()
            .len()
            > 0
    }

    fn is_internal_request(
        &self,
        _config: &TokenPreludeConfig,
        headers: &MultiMap<String, String>,
    ) -> bool {
        let Some(provider) = headers.get(PROVIDER_HEADER) else {
            return false;
        };
        provider.eq(PROVIDER_INTERNAL)
    }

    fn is_private_request(
        &self,
        _config: &TokenPreludeConfig,
        headers: &MultiMap<String, String>,
    ) -> bool {
        let Some(model) = headers.get(MODEL_HEADER) else {
            return false;
        };
        model.contains(PRIVATE_MODEL_KEY)
    }

    fn only_internal_model(
        &mut self,
        _config: &TokenPreludeConfig,
        headers: &MultiMap<String, String>,
    ) -> bool {
        let Some(use_model_types) = headers.get(WQ_USE_MODEL_HEADER) else {
            return false;
        };

        let v_list = use_model_types.split(",").collect::<Vec<&str>>();
        if v_list.len() == 1 && v_list.contains(&WQ_USE_MODEL_INTERNAL) {
            return true;
        }
        false
    }
}

struct TokenPreludeRoot {
    log: Log,
    rule_matcher: SharedRuleMatcher<TokenPreludeConfig>,
}
impl TokenPreludeRoot {
    fn new() -> Self {
        let log = Log::new(PLUGIN_NAME.to_string());
        log.info("TokenPreludeRoot::new");

        TokenPreludeRoot {
            log,
            rule_matcher: Rc::new(RefCell::new(RuleMatcher::default())),
        }
    }
}

impl Context for TokenPreludeRoot {}

impl RootContext for TokenPreludeRoot {
    fn on_configure(&mut self, _plugin_configuration_size: usize) -> bool {
        self.log.info("TokenPreludeRoot::on_configure");
        on_configure(
            self,
            _plugin_configuration_size,
            self.rule_matcher.borrow_mut().deref_mut(),
            &self.log,
        )
        // if on_configure(
        //     self,
        //     _plugin_configuration_size,
        //     self.rule_matcher.borrow_mut().deref_mut(),
        //     &self.log,
        // ) {
        //     self.rule_matcher
        //         .borrow_mut()
        //         .rewrite_config(|c| TokenPreludeConfig {
        //             support_models_map: c
        //                 .support_models
        //                 .as_ref()
        //                 .unwrap_or(&Vec::new())
        //                 .iter()
        //                 .map(|m| (m.clone(), ()))
        //                 .collect(),
        //             ..c.clone()
        //         });
        // }
        // true
    }
    fn create_http_context(&self, context_id: u32) -> Option<Box<dyn HttpContext>> {
        self.log.info(&format!(
            "TokenPreludeRoot::create_http_context({})",
            context_id
        ));

        self.create_http_context_use_wrapper(context_id)
    }
    fn get_type(&self) -> Option<ContextType> {
        Some(ContextType::HttpContext)
    }
}

impl RootContextWrapper<TokenPreludeConfig> for TokenPreludeRoot {
    fn rule_matcher(&self) -> &SharedRuleMatcher<TokenPreludeConfig> {
        &self.rule_matcher
    }

    fn create_http_context_wrapper(
        &self,
        context_id: u32,
    ) -> Option<Box<dyn HttpContextWrapper<TokenPreludeConfig>>> {
        Some(Box::new(TokenPrelude {
            config: None,
            log: Log::new(PLUGIN_NAME.to_string()),
            weak: Weak::default(),
            user_context: HashMap::default(),
            cid: context_id as i64,
            // cid: -1,
        }))
    }
}
