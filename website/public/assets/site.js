// Navigation remains functional without JavaScript; close menus on Escape or an outside click.
for (const detail of document.querySelectorAll('.header details')) {
  detail.addEventListener('toggle', () => {
    if (detail.open) for (const other of document.querySelectorAll('.header details[open]')) if (other !== detail) other.open = false;
  });
}
document.addEventListener('keydown', event => {
  if (event.key === 'Escape') for (const menu of document.querySelectorAll('.header details[open]')) { menu.open = false; menu.querySelector('summary').focus(); }
});
document.addEventListener('click', event => { for (const menu of document.querySelectorAll('.header details[open]')) if (!menu.contains(event.target)) menu.open = false; });
// Copy buttons next to build commands.
for (const button of document.querySelectorAll('button.copy')) {
  button.addEventListener('click', async () => {
    let copied = false;
    try { await navigator.clipboard.writeText(button.dataset.copy); copied = true; } catch {
      // Fallback for browsers without clipboard permission: select the command and use the copy command.
      const code = button.parentElement.querySelector('code'), range = document.createRange();
      range.selectNodeContents(code); const selection = getSelection(); selection.removeAllRanges(); selection.addRange(range);
      try { copied = document.execCommand('copy'); } catch { copied = false; }
    }
    if (!copied) return;
    const label = button.textContent; button.textContent = '✓'; button.setAttribute('aria-label', button.dataset.done);
    setTimeout(() => { button.textContent = label; button.setAttribute('aria-label', button.title); }, 1600);
  });
}
