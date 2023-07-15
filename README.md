# Manage temporary directories

Use `cargo install t-rs` and put the following in your `.bashrc` or `.zshrc` file.
Then use through the `t` command/function.

```
function t() {
    cd $(t-rs $@ | tail -n 1)
}
```

Use `t --help` for an explanation of the command line options
