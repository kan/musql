// A folding group of a sidebar list (the `list-group` classes of style.css): the Redis
// key list's types and the PostgreSQL table list's schemas (#96). `folded` is the caller's
// Set of folded names, which the header's click updates. Returns { el, count, body }: the
// group, its count (the caller fills it in) and the element its items go in.
function createListGroup(name, folded) {
  const el = document.createElement("div");
  el.className = "list-group";
  el.classList.toggle("collapsed", folded.has(name));
  const header = document.createElement("div");
  header.className = "list-group-header";
  const toggle = document.createElement("span");
  toggle.className = "toggle";
  header.appendChild(toggle);
  const label = document.createElement("span");
  label.textContent = name;
  header.appendChild(label);
  const count = document.createElement("span");
  count.className = "group-count";
  header.appendChild(count);
  header.addEventListener("click", () => {
    const fold = !folded.has(name);
    if (fold) folded.add(name);
    else folded.delete(name);
    el.classList.toggle("collapsed", fold);
  });
  el.appendChild(header);
  const body = document.createElement("div");
  body.className = "list-group-items";
  el.appendChild(body);
  return { el, count, body };
}

window.createListGroup = createListGroup;
