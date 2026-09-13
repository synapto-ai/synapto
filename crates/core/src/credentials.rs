use synapto_interface::credentials::{
    CredentialsBuilder, CredentialsHandle, PluggableCredentialsProvider,
};

pub trait CredentialsTuple: Send + Sync + 'static {
    fn build_handle<C: crate::config::ConfigProvider>(
        config_provider: &C,
    ) -> Result<CredentialsHandle, String>;
}

impl CredentialsTuple for () {
    fn build_handle<C: crate::config::ConfigProvider>(
        _config_provider: &C,
    ) -> Result<CredentialsHandle, String> {
        Ok(CredentialsHandle::default())
    }
}

macro_rules! impl_credentials_tuple {
    ($($T:ident),+) => {
        impl<$($T: PluggableCredentialsProvider),+> CredentialsTuple for ($($T,)+) {
            fn build_handle<C: crate::config::ConfigProvider>(
                config_provider: &C,
            ) -> Result<CredentialsHandle, String> {
                let mut builder = CredentialsBuilder::default();
                $(
                    let full_path = std::any::type_name::<$T>();
                    let crate_name = full_path.split("::").next().unwrap_or("").replace('-', "_");
                    let type_name = full_path.split("::").last().unwrap_or("");
                    let config: <$T as PluggableCredentialsProvider>::Config =
                        config_provider.get_credentials_config(&crate_name, type_name);
                    $T::register_provider(config, &mut builder)?;
                )+
                Ok(builder.build())
            }
        }
    };
}

impl_credentials_tuple!(P1);
impl_credentials_tuple!(P1, P2);
impl_credentials_tuple!(P1, P2, P3);
impl_credentials_tuple!(P1, P2, P3, P4);
impl_credentials_tuple!(P1, P2, P3, P4, P5);
impl_credentials_tuple!(P1, P2, P3, P4, P5, P6);
