// The busy bar fills left to right over the time the same action took last time (p.exp), then creeps toward the end
// if it runs longer. p.el: seconds since the action started; p is null while it waits for its turn.
let progAt = null, progExp = 30, progTimer = 0, progHide = 0;
function progTick() {
  const bar = document.getElementById('prog');
  const f = progAt === null ? 0 : (Date.now() - progAt) / 1000 / progExp;
  const w = f < 0.9 ? f : 0.9 + 0.09 * (1 - Math.exp(-(f - 0.9) * 3));
  bar.style.width = (w * 100).toFixed(1) + '%';
}
function drawProg(busy, p) {
  const bar = document.getElementById('prog');
  if (!busy) {
    if (!progTimer) { bar.hidden = true; return; }
    clearInterval(progTimer); progTimer = 0;
    bar.style.width = '100%';
    progHide = setTimeout(() => { bar.hidden = true; bar.style.width = '0%'; }, 400);
    return;
  }
  clearTimeout(progHide);
  progAt = p ? Date.now() - p.el * 1000 : null;
  progExp = p ? Math.max(p.exp, 1) : 30;
  bar.hidden = false;
  progTick();
  if (!progTimer) progTimer = setInterval(progTick, 200);
}
