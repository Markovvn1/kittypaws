mod python_plugin;
use python_plugin::load as load_py_plugin;

use std::ops::Range;
use rand::*;
use std::thread::JoinHandle;
use std::thread;
use std::sync::Mutex;
use std::path::PathBuf;
use std::collections::HashMap;
use std::time::Duration;
use iso8601_duration::Duration as ISODuration;
use gag::BufferRedirect;
use crate::settings::PluginsConfig;
use crate::stdout_styling::style_line;
use std::io::Read;
use lazy_static::lazy_static;


const PLUGINS_PATH: &str = "~/.kittypaws/plugins";
pub type CallablePlugin = Box<dyn PluginInterface + Send + Sync + 'static>;
lazy_static! {
    static ref STDOUT_MUTEX: Mutex<()> = Mutex::new(());
}


#[derive(Debug)]
pub enum PluginLanguage {
    PYTHON
}

pub trait PluginInterface {
    fn run(&self, config: &HashMap<String, String>) -> Result<(), String>;
}


pub struct PluginsRunner;


#[derive(Debug)]
pub enum Frequency {
    Fixed(Duration),
    Random(Range<Duration>),
    Once
}

trait FromConfig<T> {
    fn from_config(config: &HashMap<String, String>) -> Result<T, String>;
}

impl FromConfig<Frequency> for Frequency {
    fn from_config(config: &HashMap<String, String>) -> Result<Frequency, String> {
        if !config.contains_key("frequency") {
            return Ok(Frequency::Once);
        }

        match config.get("frequency").unwrap().to_lowercase().as_str() {
            "once" => Ok(Frequency::Once),
            "fixed" => {
                if let Some(interval) = config.get("interval") {
                    let interval = ISODuration::parse(interval).expect("interval has ISO8601 format").to_std();
                    return Ok(Frequency::Fixed(interval));
                }
                Err("Can't find interval in config".to_string())

            },
            "random" => {
                let min_interval = config.get("min_interval").expect("Config has min_interval");
                let max_interval = config.get("max_interval").expect("Config has max_interval");
                let min_duration = ISODuration::parse(min_interval).expect("min_interval has ISO8601 format").to_std();
                let max_duration = ISODuration::parse(max_interval).expect("min_interval has ISO8601 format").to_std();

                if min_duration > max_duration {
                    return Err(format!("min_interval {} should be less or equal than max_interval {}", &min_interval, &min_interval));
                }


                Ok(Frequency::Random(min_duration..max_duration))
            }
            _ => Ok(Frequency::Once)
        }
    }
}


impl PluginsRunner {
    fn run_plugin(&self, name: String, plugin: CallablePlugin, config: HashMap<String, String>) -> JoinHandle<()> {
        let frequency = Frequency::from_config(&config).expect("Frequency is properly configured");
        return thread::spawn(move || {
            loop {
                let _l = STDOUT_MUTEX.lock().unwrap();
                let mut buf = BufferRedirect::stdout().expect(&format!("Can't redirect STDOUT of {} plugin", name));

                match plugin.run(&config) {
                    Err(err) => panic!("Error while running plugin {}: {}", name, err),
                    _ => {}
                }

                let mut output = String::new();
                buf.read_to_string(&mut output).expect(&format!("Problem reading stdout of {} plugin", name));

                drop(buf);
                let output = style_line(name.to_string(), output);
                drop(_l);
                print!("{}", output);

                match &frequency {
                    Frequency::Once => break,
                    Frequency::Fixed(duration) => thread::sleep(*duration),
                    Frequency::Random(range) => thread::sleep(
                        Duration::from_secs(
                        rand::thread_rng()
                            .gen_range(range.start.as_secs()..range.end.as_secs())
                        )
                    )
                }
            }
        })
    }

    pub fn run(&mut self, config: &PluginsConfig) {
        let mut handles: Vec<JoinHandle<()>> = Vec::new();

        for plugconf in config {
            let name = plugconf.keys().last().expect("Name of the plugin is not specified properly in the config");
            let plugconfig = plugconf.get(name).expect(&format!("Can't parse config for plugin {}", &name));

            match load_plugin(&name) {
                Ok(plugin) => {
                    let plugconfig = plugconfig.clone();
                    let thread = self.run_plugin(name.to_string(), plugin, plugconfig);

                    handles.push(thread);
                }
                Err(err) => println!("! WARNING: {}", err)
            }
        }


        for handle in handles {
            match handle.join() {
                Err(e) => {
                    println!("Error: {:?}", e);
                },
                _ => {}
            }
        }
    }
}


fn unwrap_home_path(path: &str) -> PathBuf {
    if path.starts_with("~") {
        match std::env::var("HOME") {
            Ok(home) => {
                PathBuf::from(&path.replace("~", &home))
            }
            Err(_) => {
                println!("Could not find home directory");
                PathBuf::from(path)
            }
        }
    }
    else {
        PathBuf::from(path)
    }
}


fn detect_language(_: &str) -> PluginLanguage {
    return PluginLanguage::PYTHON;
}


fn load_plugin(name: &str) -> Result<CallablePlugin, String> {
    match detect_language(name) {
        PluginLanguage::PYTHON => load_py_plugin(&name)
    }
}
