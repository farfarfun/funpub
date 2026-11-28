//! funpub 的 Python 扩展模块（PyO3）。
//!
//! 将 `funpub-core` 的全部能力暴露给 Python，Python 侧零运行时依赖。

use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

use funpub_core::{
    ensure_parent_dir as core_ensure_parent_dir, get_filepath as core_get_filepath,
    normalize_channel_name, AliyunClient as CoreAliyunClient, AliyunConfig,
    AliyunPublisher as CoreAliyunPublisher, DownloadTarget, FunPubError as CoreError,
    PublishResult as CorePublishResult, Publisher, TimeoutConfig, UploadOptions,
    DEFAULT_CHUNK_SIZE, DEFAULT_MERGE_POLL_INTERVAL, DEFAULT_MERGE_TIMEOUT, DEFAULT_REPO_TYPE,
};
use pyo3::create_exception;
use pyo3::exceptions::{
    PyAttributeError, PyException, PyFileExistsError, PyFileNotFoundError, PyIsADirectoryError,
    PyKeyError, PyNotImplementedError, PyOSError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule, PyTuple};
use serde_json::Value;

// 异常继承树：
// Exception -> FunPubError -> PublishError -> 各具体异常
create_exception!(_funpub, FunPubError, PyException);
create_exception!(_funpub, PublishError, FunPubError);
create_exception!(_funpub, AuthenticationError, PublishError);
create_exception!(_funpub, RemoteNotFoundError, PublishError);
create_exception!(_funpub, ChecksumMismatchError, PublishError);
create_exception!(_funpub, MergeTimeoutError, PublishError);
create_exception!(_funpub, ChannelNotFoundError, PublishError);
create_exception!(_funpub, UploadError, PublishError);
create_exception!(_funpub, DownloadError, PublishError);
create_exception!(_funpub, HttpError, PublishError);
create_exception!(_funpub, ConfigError, PublishError);

fn to_py_err(err: CoreError) -> PyErr {
    match err {
        CoreError::AuthenticationError(msg) => AuthenticationError::new_err(msg),
        CoreError::NotFoundError(msg) => RemoteNotFoundError::new_err(msg),
        CoreError::ChecksumMismatchError(msg) => ChecksumMismatchError::new_err(msg),
        CoreError::MergeTimeoutError(msg) => MergeTimeoutError::new_err(msg),
        CoreError::ChannelNotFoundError(msg) => ChannelNotFoundError::new_err(msg),
        CoreError::UploadError(msg) => UploadError::new_err(msg),
        CoreError::DownloadError(msg) => DownloadError::new_err(msg),
        CoreError::HttpError { message, .. } => HttpError::new_err(message),
        CoreError::ConfigError(msg) => ConfigError::new_err(msg),
        CoreError::InvalidArgument(msg) => PyValueError::new_err(msg),
        CoreError::FileNotFound(path) => PyFileNotFoundError::new_err(path.display().to_string()),
        CoreError::FileExists(msg) => PyFileExistsError::new_err(msg),
        CoreError::IsADirectory(path) => PyIsADirectoryError::new_err(format!(
            "目标路径是目录，不能作为文件写入: {}",
            path.display()
        )),
        CoreError::SecretError(msg) => PublishError::new_err(msg),
        CoreError::Io(io_err) => match io_err.kind() {
            ErrorKind::NotFound => PyFileNotFoundError::new_err(io_err.to_string()),
            ErrorKind::AlreadyExists => PyFileExistsError::new_err(io_err.to_string()),
            ErrorKind::IsADirectory => PyIsADirectoryError::new_err(io_err.to_string()),
            _ => PyOSError::new_err(io_err.to_string()),
        },
    }
}

fn json_value_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok(b.into_pyobject(py)?.to_owned().into_any().unbind()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into_any().unbind())
            } else if let Some(u) = n.as_u64() {
                Ok(u.into_pyobject(py)?.into_any().unbind())
            } else {
                let f = n.as_f64().unwrap_or(0.0);
                Ok(f.into_pyobject(py)?.into_any().unbind())
            }
        }
        Value::String(s) => Ok(s.into_pyobject(py)?.into_any().unbind()),
        Value::Array(arr) => {
            let list = PyList::empty(py);
            for item in arr {
                list.append(json_value_to_py(py, item)?)?;
            }
            Ok(list.into_any().unbind())
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, json_value_to_py(py, v)?)?;
            }
            Ok(dict.into_any().unbind())
        }
    }
}

fn json_map_to_py_dict(
    py: Python<'_>,
    map: &serde_json::Map<String, Value>,
) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for (k, v) in map {
        dict.set_item(k, json_value_to_py(py, v)?)?;
    }
    Ok(dict.unbind())
}

fn parse_timeout(timeout: Option<&Bound<'_, PyAny>>) -> PyResult<TimeoutConfig> {
    let Some(obj) = timeout else {
        return Ok(TimeoutConfig::default());
    };
    if obj.is_none() {
        return Ok(TimeoutConfig::default());
    }
    if let Ok(secs) = obj.extract::<f64>() {
        return TimeoutConfig::from_secs_f64(secs).map_err(to_py_err);
    }
    if let Ok(tuple) = obj.cast::<PyTuple>() {
        if tuple.len() != 2 {
            return Err(PyValueError::new_err(
                "timeout 元组必须包含两个大于 0 的数值",
            ));
        }
        let connect = tuple.get_item(0)?.extract::<f64>()?;
        let read = tuple.get_item(1)?.extract::<f64>()?;
        return TimeoutConfig::from_pair_secs_f64(connect, read).map_err(to_py_err);
    }
    Err(PyTypeError::new_err(
        "timeout 必须是数值或 (connect, read) 二元组",
    ))
}

fn parse_positive_duration(secs: f64, name: &str) -> PyResult<Duration> {
    if !secs.is_finite() || secs <= 0.0 {
        return Err(PyValueError::new_err(format!("{name} 必须大于 0")));
    }
    Ok(Duration::from_secs_f64(secs))
}

fn parse_non_negative_duration(secs: f64, name: &str) -> PyResult<Duration> {
    if !secs.is_finite() || secs < 0.0 {
        return Err(PyValueError::new_err(format!("{name} 不能小于 0")));
    }
    Ok(Duration::from_secs_f64(secs))
}

/// 根据 `(filedir, filename)` 或 `filepath` 计算完整的本地文件路径。
#[pyfunction]
#[pyo3(signature = (filedir = None, filename = None, filepath = None))]
fn get_filepath(
    filedir: Option<PathBuf>,
    filename: Option<String>,
    filepath: Option<PathBuf>,
) -> PyResult<String> {
    let path = core_get_filepath(filedir.as_deref(), filename.as_deref(), filepath.as_deref())
        .map_err(to_py_err)?;
    Ok(path.to_string_lossy().into_owned())
}

/// 确保 `path` 的父目录存在，返回 `path` 本身。
#[pyfunction]
fn ensure_parent_dir(path: PathBuf) -> PyResult<String> {
    let out = core_ensure_parent_dir(&path).map_err(to_py_err)?;
    Ok(out.to_string_lossy().into_owned())
}

/// 一次上传操作的结果，支持属性访问与字典式访问（`[]` / `get` / `keys` / `in` / `to_dict`）。
#[pyclass(name = "PublishResult")]
struct PyPublishResult {
    data: Py<PyDict>,
}

impl PyPublishResult {
    fn from_core(py: Python<'_>, res: CorePublishResult) -> PyResult<Self> {
        let dict = PyDict::new(py);
        dict.set_item("path", res.path)?;
        dict.set_item("version", res.version)?;
        dict.set_item("url", res.url)?;
        dict.set_item("size", res.size)?;
        dict.set_item("md5", res.md5)?;
        dict.set_item("sha1", res.sha1)?;
        dict.set_item("sha256", res.sha256)?;
        dict.set_item("filename", res.filename)?;
        for (k, v) in res.ext {
            dict.set_item(k, json_value_to_py(py, &v)?)?;
        }
        Ok(Self {
            data: dict.unbind(),
        })
    }
}

#[pymethods]
impl PyPublishResult {
    #[new]
    #[pyo3(signature = (
        path,
        version,
        url = None,
        size = None,
        md5 = None,
        sha1 = None,
        sha256 = None,
        filename = None,
        ext = None,
        **kwargs
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        path: String,
        version: String,
        url: Option<String>,
        size: Option<u64>,
        md5: Option<String>,
        sha1: Option<String>,
        sha256: Option<String>,
        filename: Option<String>,
        ext: Option<&Bound<'_, PyDict>>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let dict = PyDict::new(py);
        dict.set_item("path", path)?;
        dict.set_item("version", version)?;
        dict.set_item("url", url)?;
        dict.set_item("size", size)?;
        dict.set_item("md5", md5)?;
        dict.set_item("sha1", sha1)?;
        dict.set_item("sha256", sha256)?;
        dict.set_item("filename", filename)?;
        if let Some(extra) = ext {
            for (k, v) in extra.iter() {
                dict.set_item(k, v)?;
            }
        }
        if let Some(kw) = kwargs {
            for (k, v) in kw.iter() {
                dict.set_item(k, v)?;
            }
        }
        Ok(Self {
            data: dict.unbind(),
        })
    }

    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        let dict = self.data.bind(py);
        match dict.get_item(name)? {
            Some(val) => Ok(val.unbind()),
            None => Err(PyAttributeError::new_err(name.to_string())),
        }
    }

    fn __setattr__(&self, py: Python<'_>, name: &str, value: Py<PyAny>) -> PyResult<()> {
        self.data.bind(py).set_item(name, value)
    }

    fn __delattr__(&self, py: Python<'_>, name: &str) -> PyResult<()> {
        let dict = self.data.bind(py);
        if dict.contains(name)? {
            dict.del_item(name)
        } else {
            Err(PyAttributeError::new_err(name.to_string()))
        }
    }

    fn __getitem__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let dict = self.data.bind(py);
        match dict.get_item(key)? {
            Some(val) => Ok(val.unbind()),
            None => Err(PyKeyError::new_err(key.clone().unbind())),
        }
    }

    fn __setitem__(
        &self,
        py: Python<'_>,
        key: &Bound<'_, PyAny>,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        self.data.bind(py).set_item(key, value)
    }

    fn __delitem__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<()> {
        self.data.bind(py).del_item(key)
    }

    fn __contains__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.data.bind(py).contains(key)
    }

    fn __len__(&self, py: Python<'_>) -> usize {
        self.data.bind(py).len()
    }

    fn __iter__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.data
            .bind(py)
            .as_any()
            .call_method0("__iter__")
            .map(|b| b.unbind())
    }

    #[pyo3(signature = (key, default = None))]
    fn get(
        &self,
        py: Python<'_>,
        key: &Bound<'_, PyAny>,
        default: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let dict = self.data.bind(py);
        match dict.get_item(key)? {
            Some(val) => Ok(val.unbind()),
            None => Ok(default.unwrap_or_else(|| py.None())),
        }
    }

    fn keys(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.data
            .bind(py)
            .as_any()
            .call_method0("keys")
            .map(|b| b.unbind())
    }

    fn values(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.data
            .bind(py)
            .as_any()
            .call_method0("values")
            .map(|b| b.unbind())
    }

    fn items(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.data
            .bind(py)
            .as_any()
            .call_method0("items")
            .map(|b| b.unbind())
    }

    fn to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        self.data.bind(py).copy().map(|d| d.unbind())
    }

    fn __eq__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<bool> {
        let self_dict = self.data.bind(py);
        if let Ok(other_res) = other.cast::<PyPublishResult>() {
            return self_dict
                .as_any()
                .eq(other_res.borrow().data.bind(py).as_any());
        }
        if let Ok(other_dict) = other.cast::<PyDict>() {
            return self_dict.as_any().eq(other_dict.as_any());
        }
        Ok(false)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let dict = self.data.bind(py);
        let path = match dict.get_item("path")? {
            Some(v) => v.repr()?.to_str()?.to_string(),
            None => "None".to_string(),
        };
        let version = match dict.get_item("version")? {
            Some(v) => v.repr()?.to_str()?.to_string(),
            None => "None".to_string(),
        };
        let url = match dict.get_item("url")? {
            Some(v) => v.repr()?.to_str()?.to_string(),
            None => "None".to_string(),
        };
        Ok(format!(
            "PublishResult(path={path}, version={version}, url={url})"
        ))
    }
}

/// 发布渠道基类，供 Python 子类扩展自定义发布渠道。
#[pyclass(name = "BasePublisher", subclass)]
struct PyBasePublisher;

#[pymethods]
impl PyBasePublisher {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        Self
    }

    #[pyo3(signature = (_filepath, _path, _version, _filename = None, _description = None, _overwrite = false, *_args, **_kwargs))]
    #[allow(clippy::too_many_arguments)]
    fn upload_file(
        &self,
        _filepath: &Bound<'_, PyAny>,
        _path: &str,
        _version: &str,
        _filename: Option<&str>,
        _description: Option<&str>,
        _overwrite: bool,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        Err(PyNotImplementedError::new_err(()))
    }

    #[pyo3(signature = (_path, _version, _save_dir = None, _filename = None, _filepath = None, _overwrite = false, *_args, **_kwargs))]
    #[allow(clippy::too_many_arguments)]
    fn download_file(
        &self,
        _path: &str,
        _version: &str,
        _save_dir: Option<&Bound<'_, PyAny>>,
        _filename: Option<&str>,
        _filepath: Option<&Bound<'_, PyAny>>,
        _overwrite: bool,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<bool> {
        Err(PyNotImplementedError::new_err(()))
    }

    #[pyo3(signature = (_path, _version, *_args, **_kwargs))]
    fn exist(
        &self,
        _path: &str,
        _version: &str,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<bool> {
        Err(PyNotImplementedError::new_err(()))
    }

    #[pyo3(signature = (_path, _version, _expiration = None, *_args, **_kwargs))]
    fn get_download_url(
        &self,
        _path: &str,
        _version: &str,
        _expiration: Option<u64>,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        Err(PyNotImplementedError::new_err(()))
    }

    fn close(&self) {}

    fn __enter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_exc))]
    fn __exit__(slf: Bound<'_, Self>, _exc: &Bound<'_, PyTuple>) -> PyResult<()> {
        slf.as_any().call_method0("close")?;
        Ok(())
    }
}

/// 阿里云 Packages Generic 仓库底层 REST 客户端。
#[pyclass(name = "AliyunClient")]
struct PyAliyunClient {
    inner: CoreAliyunClient,
}

#[pymethods]
impl PyAliyunClient {
    #[new]
    #[pyo3(signature = (repo_url, username, password, timeout = None))]
    fn new(
        repo_url: &str,
        username: &str,
        password: &str,
        timeout: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let timeout_cfg = parse_timeout(timeout)?;
        let inner =
            CoreAliyunClient::new(repo_url, username, password, timeout_cfg).map_err(to_py_err)?;
        Ok(Self { inner })
    }

    #[getter]
    fn repo_url(&self) -> &str {
        self.inner.repo_url()
    }

    #[getter]
    fn username(&self) -> &str {
        self.inner.username()
    }

    #[getter]
    fn scheme(&self) -> &str {
        self.inner.scheme()
    }

    #[getter]
    fn host(&self) -> &str {
        self.inner.host()
    }

    #[getter]
    fn org_id(&self) -> &str {
        self.inner.org_id()
    }

    #[getter]
    fn repo_id(&self) -> &str {
        self.inner.repo_id()
    }

    #[pyo3(signature = (filepath, path, version, filename = None, description = None))]
    fn simple_upload(
        &self,
        py: Python<'_>,
        filepath: PathBuf,
        path: String,
        version: String,
        filename: Option<String>,
        description: Option<String>,
    ) -> PyResult<Py<PyDict>> {
        let client = self.inner.clone();
        let obj = py
            .detach(move || {
                client.simple_upload(
                    &filepath,
                    &path,
                    &version,
                    filename.as_deref(),
                    description.as_deref(),
                )
            })
            .map_err(to_py_err)?;
        json_map_to_py_dict(py, &obj)
    }

    #[pyo3(signature = (path, version, dest_path, chunk_size = 8 * 1024 * 1024))]
    fn download(
        &self,
        py: Python<'_>,
        path: String,
        version: String,
        dest_path: PathBuf,
        chunk_size: i64,
    ) -> PyResult<()> {
        if chunk_size <= 0 {
            return Err(PyValueError::new_err("chunk_size 必须大于 0"));
        }
        let client = self.inner.clone();
        py.detach(move || client.download(&path, &version, &dest_path, chunk_size as usize))
            .map_err(to_py_err)
    }

    fn exist(&self, py: Python<'_>, path: String, version: String) -> PyResult<bool> {
        let client = self.inner.clone();
        py.detach(move || client.exist(&path, &version))
            .map_err(to_py_err)
    }

    fn get_signed_download_url(
        &self,
        py: Python<'_>,
        path: String,
        version: String,
        expiration: u64,
    ) -> PyResult<String> {
        let client = self.inner.clone();
        py.detach(move || client.get_signed_download_url(&path, &version, expiration))
            .map_err(to_py_err)
    }

    #[pyo3(signature = (
        filepath,
        path,
        version,
        filename = None,
        description = None,
        chunk_size = DEFAULT_CHUNK_SIZE as i64,
        poll_interval = DEFAULT_MERGE_POLL_INTERVAL.as_secs_f64(),
        poll_timeout = DEFAULT_MERGE_TIMEOUT.as_secs_f64(),
    ))]
    #[allow(clippy::too_many_arguments)]
    fn chunked_upload(
        &self,
        py: Python<'_>,
        filepath: PathBuf,
        path: String,
        version: String,
        filename: Option<String>,
        description: Option<String>,
        chunk_size: i64,
        poll_interval: f64,
        poll_timeout: f64,
    ) -> PyResult<Py<PyDict>> {
        if chunk_size <= 0 {
            return Err(PyValueError::new_err("chunk_size 必须大于 0"));
        }
        let poll_interval_dur = parse_positive_duration(poll_interval, "poll_interval")?;
        let poll_timeout_dur = parse_non_negative_duration(poll_timeout, "poll_timeout")?;
        let client = self.inner.clone();
        let obj = py
            .detach(move || {
                client.chunked_upload(
                    &filepath,
                    &path,
                    &version,
                    filename.as_deref(),
                    description.as_deref(),
                    chunk_size as usize,
                    poll_interval_dur,
                    poll_timeout_dur,
                )
            })
            .map_err(to_py_err)?;
        json_map_to_py_dict(py, &obj)
    }

    fn close(&self) {}

    fn __enter__(slf: Bound<'_, Self>) -> Bound<'_, Self> {
        slf
    }

    #[pyo3(signature = (*_exc))]
    fn __exit__(&self, _exc: &Bound<'_, PyTuple>) {}

    fn __repr__(&self) -> String {
        format!(
            "AliyunClient(repo_url={:?}, username={:?})",
            self.inner.repo_url(),
            self.inner.username()
        )
    }
}

/// 阿里云 Packages Generic 制品仓库发布器。
#[pyclass(name = "AliyunPublisher", extends = PyBasePublisher, subclass)]
struct PyAliyunPublisher {
    inner: CoreAliyunPublisher,
}

#[pymethods]
impl PyAliyunPublisher {
    #[new]
    #[pyo3(signature = (
        repo_name = None,
        repo_type = DEFAULT_REPO_TYPE,
        repo_url = None,
        username = None,
        password = None,
        chunk_size = DEFAULT_CHUNK_SIZE as i64,
        chunk_threshold = None,
        timeout = None,
        *_args,
        **_kwargs
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        repo_name: Option<String>,
        repo_type: &str,
        repo_url: Option<String>,
        username: Option<String>,
        password: Option<String>,
        chunk_size: i64,
        chunk_threshold: Option<i64>,
        timeout: Option<&Bound<'_, PyAny>>,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        if chunk_size <= 0 {
            return Err(PyValueError::new_err("chunk_size 必须大于 0"));
        }
        let parsed_threshold = match chunk_threshold {
            Some(v) if v < 0 => {
                return Err(PyValueError::new_err("chunk_threshold 不能小于 0"));
            }
            Some(v) => Some(v as usize),
            None => None,
        };
        let timeout_cfg = parse_timeout(timeout)?;
        let cfg = AliyunConfig {
            repo_name,
            repo_type: repo_type.to_string(),
            repo_url,
            username,
            password,
            chunk_size: chunk_size as usize,
            chunk_threshold: parsed_threshold,
            timeout: timeout_cfg,
            secret_dir: None,
        };
        let inner = py
            .detach(move || CoreAliyunPublisher::new(cfg))
            .map_err(to_py_err)?;
        Ok(PyClassInitializer::from(PyBasePublisher).add_subclass(Self { inner }))
    }

    #[getter]
    fn chunk_size(&self) -> usize {
        self.inner.chunk_size()
    }

    #[getter]
    fn chunk_threshold(&self) -> usize {
        self.inner.chunk_threshold()
    }

    #[getter]
    fn client(&self) -> PyAliyunClient {
        PyAliyunClient {
            inner: self.inner.client().clone(),
        }
    }

    #[getter]
    fn repo_url(&self) -> &str {
        self.inner.client().repo_url()
    }

    #[getter]
    fn username(&self) -> &str {
        self.inner.client().username()
    }

    #[pyo3(signature = (
        filepath,
        path,
        version,
        filename = None,
        description = None,
        overwrite = false,
        chunked = None,
        poll_interval = None,
        poll_timeout = None,
        *_args,
        **_kwargs
    ))]
    #[allow(clippy::too_many_arguments)]
    fn upload_file(
        &self,
        py: Python<'_>,
        filepath: PathBuf,
        path: String,
        version: String,
        filename: Option<String>,
        description: Option<String>,
        overwrite: bool,
        chunked: Option<bool>,
        poll_interval: Option<f64>,
        poll_timeout: Option<f64>,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyPublishResult> {
        let poll_interval_dur = match poll_interval {
            Some(v) => Some(parse_positive_duration(v, "poll_interval")?),
            None => None,
        };
        let poll_timeout_dur = match poll_timeout {
            Some(v) => Some(parse_non_negative_duration(v, "poll_timeout")?),
            None => None,
        };
        let options = UploadOptions {
            filename,
            description,
            overwrite,
            chunked,
            poll_interval: poll_interval_dur,
            poll_timeout: poll_timeout_dur,
        };
        let publisher = self.inner.clone();
        let res = py
            .detach(move || publisher.upload_file(&filepath, &path, &version, &options))
            .map_err(to_py_err)?;
        PyPublishResult::from_core(py, res)
    }

    #[pyo3(signature = (
        path,
        version,
        save_dir = None,
        filename = None,
        filepath = None,
        overwrite = false,
        *_args,
        **_kwargs
    ))]
    #[allow(clippy::too_many_arguments)]
    fn download_file(
        &self,
        py: Python<'_>,
        path: String,
        version: String,
        save_dir: Option<PathBuf>,
        filename: Option<String>,
        filepath: Option<PathBuf>,
        overwrite: bool,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<bool> {
        let target = DownloadTarget {
            save_dir,
            filename,
            filepath,
            overwrite,
        };
        let publisher = self.inner.clone();
        py.detach(move || publisher.download_file(&path, &version, &target))
            .map_err(to_py_err)?;
        Ok(true)
    }

    #[pyo3(signature = (path, version, *_args, **_kwargs))]
    fn exist(
        &self,
        py: Python<'_>,
        path: String,
        version: String,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<bool> {
        let publisher = self.inner.clone();
        py.detach(move || publisher.exist(&path, &version))
            .map_err(to_py_err)
    }

    #[pyo3(signature = (path, version, expiration = None, *_args, **_kwargs))]
    fn get_download_url(
        &self,
        py: Python<'_>,
        path: String,
        version: String,
        expiration: Option<u64>,
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        let publisher = self.inner.clone();
        py.detach(move || publisher.get_download_url(&path, &version, expiration))
            .map_err(to_py_err)
    }

    fn close(&self) {}

    fn __repr__(&self) -> String {
        format!(
            "AliyunPublisher(repo_url={:?}, username={:?})",
            self.inner.client().repo_url(),
            self.inner.client().username()
        )
    }
}

/// 根据渠道名称获取一个 Publisher 实例。
#[pyfunction]
#[pyo3(signature = (channel, *args, **kwargs))]
fn get_publisher(
    py: Python<'_>,
    channel: &str,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    let key = normalize_channel_name(channel);
    match key.as_str() {
        "aliyun" => {
            let cls = py.get_type::<PyAliyunPublisher>();
            let inst = cls.call(args, kwargs)?;
            Ok(inst.unbind())
        }
        _ => Err(ChannelNotFoundError::new_err(format!(
            "不支持的发布渠道: {channel}. 可用渠道: aliyun"
        ))),
    }
}

/// 列出当前环境中可用的渠道及其 Publisher 类。
#[pyfunction]
fn list_available_channels(py: Python<'_>) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("aliyun", py.get_type::<PyAliyunPublisher>())?;
    Ok(dict.unbind())
}

/// 列出缺失依赖的渠道及其安装提示（Rust 原生扩展内置全部渠道，默认为空字典）。
#[pyfunction]
fn list_missing_channels(py: Python<'_>) -> PyResult<Py<PyDict>> {
    Ok(PyDict::new(py).unbind())
}

#[pymodule]
fn _funpub(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    let _ = pyo3_log::try_init();

    // 异常类
    module.add("FunPubError", py.get_type::<FunPubError>())?;
    module.add("PublishError", py.get_type::<PublishError>())?;
    module.add("AuthenticationError", py.get_type::<AuthenticationError>())?;
    module.add("RemoteNotFoundError", py.get_type::<RemoteNotFoundError>())?;
    module.add("NotFoundError", py.get_type::<RemoteNotFoundError>())?;
    module.add(
        "ChecksumMismatchError",
        py.get_type::<ChecksumMismatchError>(),
    )?;
    module.add("MergeTimeoutError", py.get_type::<MergeTimeoutError>())?;
    module.add(
        "ChannelNotFoundError",
        py.get_type::<ChannelNotFoundError>(),
    )?;
    module.add("UploadError", py.get_type::<UploadError>())?;
    module.add("DownloadError", py.get_type::<DownloadError>())?;
    module.add("HttpError", py.get_type::<HttpError>())?;
    module.add("ConfigError", py.get_type::<ConfigError>())?;

    // 类型
    module.add_class::<PyPublishResult>()?;
    module.add_class::<PyBasePublisher>()?;
    module.add_class::<PyAliyunClient>()?;
    module.add_class::<PyAliyunPublisher>()?;

    // 函数
    module.add_function(wrap_pyfunction!(get_publisher, module)?)?;
    module.add_function(wrap_pyfunction!(list_available_channels, module)?)?;
    module.add_function(wrap_pyfunction!(list_missing_channels, module)?)?;
    module.add_function(wrap_pyfunction!(get_filepath, module)?)?;
    module.add_function(wrap_pyfunction!(ensure_parent_dir, module)?)?;

    // 常量
    module.add("DEFAULT_CHUNK_SIZE", DEFAULT_CHUNK_SIZE)?;
    module.add("DEFAULT_REPO_TYPE", DEFAULT_REPO_TYPE)?;

    Ok(())
}
