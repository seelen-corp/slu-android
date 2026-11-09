pub struct AppError {
    msg: String,
}

macro_rules! define_app_errors {
    ($(
        $variant:ident($error_type:ty);
    )*) => {
        $(
            impl From<$error_type> for AppError {
                fn from(err: $error_type) -> Self {
                    AppError { msg: format!("{}({:?})", stringify!($variant), err) }
                }
            }
        )*
    };
}

define_app_errors!(
    App(String);
    Io(std::io::Error);
    Jni(jni::errors::Error);
    Platform(slint::PlatformError);
    SetPlatform(slint::platform::SetPlatformError);
);

impl std::fmt::Debug for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for AppError {}

impl From<&str> for AppError {
    fn from(err: &str) -> Self {
        err.to_owned().into()
    }
}

pub type Result<T = ()> = core::result::Result<T, AppError>;
