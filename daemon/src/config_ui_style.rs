pub(super) const CONFIG_UI_STYLE: &str = r#"
  <style>
    :root {{
      --bg:#ffffff; --panel:#f6f6f6; --panel2:#f7f7f7; --panel3:#ffffff; --line:#dddddd; --text:#1f1f1f; --muted:#6f6f6f; --accent:#ffb000; --accent-soft:rgba(255,176,0,.18); --accent-text:#1f1f1f;
    }}
    * {{ box-sizing:border-box; }}
    body {{
      margin:0; background:var(--bg); color:var(--text);
      font-family:Inter, "Segoe UI", -apple-system, BlinkMacSystemFont, Arial, sans-serif;
      font-size:16px;
    }}
    .layout {{ display:grid; grid-template-columns:324px minmax(0, 1fr); min-height:100vh; }}
    .sidebar {{ background:var(--panel2); border-right:1px solid var(--line); padding:30px 20px 24px; overflow:auto; }}
    .main {{ padding:48px 56px 40px; max-width:920px; width:100%; }}
    .title {{ color:var(--muted); font-size:16px; font-weight:700; margin:20px 0 18px; }}
    .subtitle {{ display:none; }}
    .nav-section {{ margin:0 0 26px; }}
    .nav-section-title {{
      color:var(--muted); font-size:13px; font-weight:700; margin:0 0 8px; padding:0 12px;
    }}
    .nav-btn {{
      width:100%; text-align:left; border:0; background:transparent; color:var(--text);
      padding:8px 12px; margin-bottom:2px; border-radius:6px; cursor:pointer;
    }}
    .nav-btn:hover {{ background:rgba(0,0,0,.045); }}
    .nav-btn.active {{ background:rgba(0,0,0,.075); }}
    .nav-name {{ display:block; font-weight:400; line-height:1.25; }}
    .nav-meta {{ display:none; }}
    .page-eyebrow {{ color:var(--muted); font-size:13px; margin-bottom:8px; }}
    .page-title {{ font-size:28px; font-weight:600; line-height:1.2; margin:0 0 6px; }}
    .page-sub {{ color:var(--muted); margin:0 0 22px; max-width:720px; line-height:1.45; }}
    .tab-row {{ display:flex; gap:6px; margin:0 0 18px; flex-wrap:wrap; }}
    .tab-btn {{
      border:0; border-radius:6px; background:transparent; color:var(--muted);
      padding:7px 10px; cursor:pointer; font-size:14px; font-weight:500;
    }}
    .tab-btn:hover {{ background:rgba(0,0,0,.045); color:var(--text); }}
    .tab-btn.active {{ color:var(--text); background:var(--accent-soft); }}
    .card {{ background:var(--panel); border:0; border-radius:8px; padding:24px; margin-bottom:14px; }}
    .card-title {{ font-size:23px; font-weight:500; margin:0 0 6px; }}
    .card-sub {{ color:var(--muted); margin:0 0 18px; font-size:16px; line-height:1.45; }}
    .input-shell {{ margin:0 0 14px; padding:0; border:1px solid transparent; border-radius:8px; transition:border-color .16s ease, background-color .16s ease, box-shadow .16s ease; }}
    .input-shell:first-of-type {{ margin-top:0; }}
    .input-shell.is-dirty {{ border-color:#b9576d; background:rgba(185,87,109,.08); box-shadow:0 0 0 1px rgba(185,87,109,.18) inset; }}
    .input-head {{ display:flex; align-items:center; justify-content:space-between; gap:12px; margin:0 0 4px; }}
    label {{ display:block; font-size:16px; font-weight:500; margin:0; }}
    .unsaved-badge {{
      display:inline-flex; align-items:center; gap:6px; border-radius:999px; padding:4px 8px;
      background:rgba(185,87,109,.16); color:#ffb7c6; font-size:12px; font-weight:700; letter-spacing:.02em;
    }}
    .unsaved-badge::before {{ content:'*'; font-size:13px; line-height:1; }}
    .field-help {{ color:var(--muted); font-size:14px; line-height:1.45; margin:0 0 10px; max-width:560px; }}
    input, select {{
      width:100%; border:1px solid var(--line); border-radius:6px; background:var(--panel3); color:var(--text);
      padding:8px 12px; font:inherit; min-height:38px;
    }}
    input:focus, select:focus, button:focus-visible {{
      outline:2px solid var(--accent-soft); outline-offset:1px; border-color:var(--accent);
    }}
    .input-shell.is-dirty input,
    .input-shell.is-dirty select,
    .input-shell.is-dirty .list-select {{
      border-color:#b9576d;
      box-shadow:0 0 0 1px rgba(185,87,109,.22);
    }}
    .btn-row {{ display:flex; gap:8px; margin-top:18px; flex-wrap:wrap; }}
    button {{
      border:1px solid var(--line); border-radius:6px; background:var(--panel3); color:var(--text); padding:8px 12px; cursor:pointer; font:inherit;
    }}
    button:hover {{ background:rgba(0,0,0,.045); }}
    button.primary {{ background:var(--accent); border-color:transparent; color:var(--accent-text); font-weight:600; }}
    button.primary:hover {{ filter:brightness(.96); }}
    button.primary.is-dirty {{ box-shadow:0 0 0 2px rgba(185,87,109,.35); }}
    [hidden] {{ display:none !important; }}
    .status-msg {{ color:var(--muted); margin-top:8px; min-height:20px; }}
    .empty {{ color:var(--muted); font-style:italic; }}
    .kv-list {{ display:grid; grid-template-columns:minmax(180px, 240px) 1fr; gap:10px 16px; }}
    .kv-key {{ color:var(--muted); }}
    .kv-value {{ word-break:break-word; }}
    .command-list {{ display:grid; gap:10px; }}
    .command-item {{ border:1px solid var(--line); border-radius:6px; padding:12px; background:var(--panel3); }}
    .command-title {{ font-weight:700; margin:0 0 4px; }}
    .command-meta {{ color:var(--muted); font-size:12px; margin:0 0 6px; }}
    .command-desc {{ color:var(--text); margin:0; font-size:14px; }}
    .command-help-title {{ color:var(--muted); font-size:12px; margin:10px 0 6px; text-transform:uppercase; letter-spacing:.06em; }}
    .command-help-list {{ margin:0; padding-left:18px; color:var(--text); font-size:13px; }}
    .command-help-list li {{ margin:0 0 4px; }}
    .mono {{ font-family:Consolas, monospace; }}
    .checkbox-list {{ display:grid; gap:8px; }}
    .toggle-control {{ display:flex; align-items:center; gap:10px; flex-wrap:wrap; }}
    .toggle-group {{ display:inline-flex; gap:8px; }}
    .toggle-btn {{ background:var(--panel3); border:1px solid var(--line); color:var(--muted); }}
    .toggle-btn.active-enable {{ background:#1f4b2f; border-color:#3d8b5c; color:#e7fff0; }}
    .toggle-btn.active-disable {{ background:#4a2222; border-color:#a25555; color:#ffecec; }}
    .toggle-state {{ color:var(--muted); font-size:13px; }}
    .extension-list {{ display:grid; gap:12px; }}
    .extension-card {{ border:1px solid var(--line); border-radius:6px; padding:14px; background:var(--panel3); }}
    .extension-card.is-dirty {{ border-color:#b9576d; box-shadow:0 0 0 1px rgba(185,87,109,.18) inset; }}
    .extension-head {{ display:flex; justify-content:space-between; gap:12px; align-items:flex-start; flex-wrap:wrap; }}
    .extension-title-row {{ display:flex; align-items:center; gap:8px; flex-wrap:wrap; }}
    .extension-name {{ font-weight:700; margin:0 0 4px; }}
    .extension-id {{ color:var(--muted); font-size:12px; }}
    .extension-meta {{ color:var(--muted); font-size:13px; margin:8px 0 0; }}
    .extension-actions {{ display:flex; gap:8px; flex-wrap:wrap; align-items:center; margin-top:12px; }}
    .mini-btn {{ padding:8px 12px; font-size:13px; }}
    .mini-btn.active-enable {{ background:#1f4b2f; border-color:#3d8b5c; color:#e7fff0; }}
    .mini-btn.active-disable {{ background:#4a2222; border-color:#a25555; color:#ffecec; }}
    .command-panel {{ margin-top:12px; display:grid; gap:10px; }}
    .list-select {{
      display:grid; gap:8px; max-height:220px; overflow:auto; padding:6px; border:1px solid var(--line);
      border-radius:6px; background:var(--panel3);
    }}
    .list-option {{
      width:100%; text-align:left; padding:10px 12px; border-radius:6px; border:1px solid var(--line);
      background:rgba(255,255,255,.01); color:var(--text);
    }}
    .list-option.active {{ border-color:var(--accent); background:var(--accent-soft); color:var(--text); }}
    .checkbox-item {{
      display:flex; gap:10px; align-items:flex-start; padding:10px 12px; border:1px solid var(--line);
      border-radius:6px; background:var(--panel3);
    }}
    .checkbox-item input {{ width:auto; margin-top:3px; }}
    .command-run-list {{ display:grid; gap:8px; }}
    .command-run-row {{
      display:flex; align-items:center; gap:14px; padding:10px 12px;
      border:1px solid var(--line); border-radius:6px; background:var(--panel3);
    }}
    .command-run-info {{ flex:1; min-width:0; }}
    .command-run-label {{ font-weight:600; }}
    .command-run-desc {{ color:var(--muted); font-size:13px; margin-top:2px; }}
    .run-btn {{
      padding:7px 16px; white-space:nowrap; font-weight:600;
      border-color:var(--accent); color:var(--accent);
    }}
    .run-btn:hover:not(:disabled) {{ background:var(--accent-soft); }}
    .run-btn:disabled {{ opacity:.55; cursor:not-allowed; }}
    .run-btn.run-ok {{ border-color:#3d8b5c; color:#3d8b5c; }}
    .run-status {{ font-size:13px; white-space:nowrap; min-width:70px; }}
    .run-status.run-ok {{ color:#3d8b5c; }}
    .run-status.run-error {{ color:#e05c6f; }}
    @media (max-width: 900px) {{
      .layout {{ grid-template-columns:1fr; }}
      .sidebar {{ border-right:none; border-bottom:1px solid var(--line); }}
      .main {{ padding:24px 18px; }}
      .kv-list {{ grid-template-columns:1fr; }}
    }}
  </style>
"#;
