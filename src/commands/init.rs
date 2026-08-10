use crate::core::repository;
use crate::ui::printer;

pub fn run() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    repository::init(&cwd)?;
    printer::ok(&format!(
        "Initialized empty prompt vault in {}",
        cwd.join(".pv").display()
    ));
    Ok(())
}
