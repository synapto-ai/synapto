use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(LLMSafe)]
pub fn llm_safe_derive(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let ast = parse_macro_input!(input as DeriveInput);

    // Get the name of the struct or enum we are deriving on
    let name = &ast.ident;

    // Extract the generics (lifetimes, type parameters, where clauses)
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();

    // Generate the Rust code for the implementation
    let expanded = quote! {
        // This generates: impl<T> LLMSafe for MyStruct<T> where T: ... {}
        impl #impl_generics LLMSafe for #name #ty_generics #where_clause {}
    };

    // Return the generated code as a TokenStream for the compiler to use
    TokenStream::from(expanded)
}