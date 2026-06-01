    function normalizeHotkeyEvent(event) {
      const normalizeKey = key => {
        const aliases = {
          ' ': 'space',
          'Spacebar': 'space',
          'Esc': 'esc',
          'Escape': 'esc',
          'Control': 'ctrl',
          'Alt': 'alt',
          'Shift': 'shift',
          'Meta': 'meta',
          'OS': 'meta',
          'ScrollLock': 'scroll_lock',
          'PageUp': 'page_up',
          'PageDown': 'page_down',
          'ArrowUp': 'arrow_up',
          'ArrowDown': 'arrow_down',
          'ArrowLeft': 'arrow_left',
          'ArrowRight': 'arrow_right'
        };
        const mapped = aliases[key] || key;
        return String(mapped).toLowerCase().replace(/\s+/g, '_');
      };

      const parts = [];
      if (event.ctrlKey) parts.push('ctrl');
      if (event.altKey) parts.push('alt');
      if (event.shiftKey) parts.push('shift');
      if (event.metaKey) parts.push('meta');

      const key = normalizeKey(event.key);
      if (key && !['ctrl', 'alt', 'shift', 'meta'].includes(key)) {
        parts.push(key);
      } else if (!parts.length && key) {
        parts.push(key);
      }
      return parts.join('+');
    }

    function createHotkeyInput(input, value) {
      const initialValue = String(value ?? input.default ?? '');
      const hidden = document.createElement('input');
      hidden.type = 'hidden';
      hidden.value = initialValue;
      hidden.dataset.inputId = input.id;
      hidden.dataset.inputType = input.type;

      const control = document.createElement('div');
      control.className = 'hotkey-control';

      const capture = document.createElement('button');
      capture.type = 'button';
      capture.className = 'hotkey-capture';

      const clearBtn = document.createElement('button');
      clearBtn.type = 'button';
      clearBtn.className = 'hotkey-clear';
      clearBtn.textContent = 'x';
      clearBtn.title = 'Clear hotkey';

      const updateLabel = () => {
        capture.textContent = hidden.value || 'Press keys';
        capture.classList.toggle('is-empty', !hidden.value);
        clearBtn.disabled = !hidden.value;
      };

      capture.addEventListener('keydown', event => {
        event.preventDefault();
        event.stopPropagation();
        const combo = normalizeHotkeyEvent(event);
        if (!combo) return;
        hidden.value = combo;
        updateLabel();
        refreshDirtyState();
      });
      clearBtn.addEventListener('click', () => {
        hidden.value = '';
        updateLabel();
        refreshDirtyState();
        capture.focus();
      });

      control.appendChild(capture);
      control.appendChild(clearBtn);
      control.appendChild(hidden);
      updateLabel();
      return { control, readValue: () => hidden.value };
    }

    function createInput(input, value, info) {
      const wrapper = document.createElement('div');
      wrapper.className = 'input-shell';
      const head = document.createElement('div');
      head.className = 'input-head';
      const label = document.createElement('label');
      label.textContent = input.label;
      const dirtyBadge = document.createElement('span');
      dirtyBadge.className = 'unsaved-badge';
      dirtyBadge.textContent = 'Unsaved';
      dirtyBadge.hidden = true;
      head.appendChild(label);
      head.appendChild(dirtyBadge);
      wrapper.appendChild(head);
      if (input.description) {
        const help = document.createElement('div');
        help.className = 'field-help';
        help.textContent = input.description;
        wrapper.appendChild(help);
      }

      let control;
      let readDirtyValue = null;
      if (input.type === 'boolean') {
        const currentValue = String(value ?? input.default ?? false);
        const hidden = document.createElement('input');
        hidden.type = 'hidden';
        hidden.value = currentValue;
        hidden.dataset.inputId = input.id;
        hidden.dataset.inputType = input.type;
        control = document.createElement('div');
        control.className = 'on-off-toggle';
        const track = document.createElement('span');
        track.className = 'on-off-toggle-track';
        const thumb = document.createElement('span');
        thumb.className = 'on-off-toggle-thumb';
        track.appendChild(thumb);
        const lbl = document.createElement('span');
        lbl.className = 'on-off-toggle-label';
        const updateToggle = () => {
          const on = hidden.value === 'true';
          track.classList.toggle('is-on', on);
          lbl.textContent = on ? 'On' : 'Off';
        };
        control.appendChild(track);
        control.appendChild(lbl);
        control.appendChild(hidden);
        updateToggle();
        control.addEventListener('click', () => {
          hidden.value = hidden.value === 'true' ? 'false' : 'true';
          updateToggle();
          refreshDirtyState();
        });
        readDirtyValue = () => hidden.value === 'true';
      } else if (input.type === 'extension-toggle') {
        const currentValue = String(value ?? input.default ?? false);
        const hidden = document.createElement('input');
        hidden.type = 'hidden';
        hidden.value = currentValue;
        hidden.dataset.inputId = input.id;
        hidden.dataset.inputType = input.type;
        hidden.dataset.valueKind = 'boolean';

        control = document.createElement('div');
        control.className = 'toggle-control';

        const buttonGroup = document.createElement('div');
        buttonGroup.className = 'toggle-group';
        const enableBtn = document.createElement('button');
        enableBtn.type = 'button';
        enableBtn.className = 'toggle-btn';
        enableBtn.textContent = 'Enable';
        const disableBtn = document.createElement('button');
        disableBtn.type = 'button';
        disableBtn.className = 'toggle-btn';
        disableBtn.textContent = 'Disable';
        const stateText = document.createElement('span');
        stateText.className = 'toggle-state';

        const updateToggleUi = () => {
          const enabled = hidden.value === 'true';
          enableBtn.className = 'toggle-btn' + (enabled ? ' active-enable' : '');
          disableBtn.className = 'toggle-btn' + (!enabled ? ' active-disable' : '');
          stateText.textContent = enabled ? 'Currently enabled' : 'Currently disabled';
        };

        enableBtn.addEventListener('click', () => {
          hidden.value = 'true';
          updateToggleUi();
          refreshDirtyState();
        });
        disableBtn.addEventListener('click', () => {
          hidden.value = 'false';
          updateToggleUi();
          refreshDirtyState();
        });

        buttonGroup.appendChild(enableBtn);
        buttonGroup.appendChild(disableBtn);
        control.appendChild(buttonGroup);
        control.appendChild(stateText);
        control.appendChild(hidden);
        updateToggleUi();
        readDirtyValue = () => hidden.value === 'true';
      } else if (input.type === 'multi-select') {
        const options = resolveInputOptions(input, info);
        const selected = Array.isArray(value)
          ? value.map(String)
          : Array.isArray(input.default)
            ? input.default.map(String)
            : [];
        const selectedSet = new Set(selected);
        control = document.createElement('div');
        control.className = 'checkbox-list';
        if (options.length === 0) {
          const empty = document.createElement('div');
          empty.className = 'empty';
          empty.textContent = 'No options available right now.';
          control.appendChild(empty);
        } else {
          options.forEach(opt => {
            const item = document.createElement('label');
            item.className = 'checkbox-item';
            const checkbox = document.createElement('input');
            checkbox.type = 'checkbox';
            checkbox.checked = selectedSet.has(opt);
            checkbox.dataset.inputId = input.id;
            checkbox.dataset.inputType = input.type;
            checkbox.dataset.optionValue = opt;
            checkbox.addEventListener('change', () => refreshDirtyState());
            const text = document.createElement('span');
            text.textContent = opt;
            item.appendChild(checkbox);
            item.appendChild(text);
            control.appendChild(item);
          });
        }
        readDirtyValue = () =>
          Array.from(control.querySelectorAll(`[data-input-id="${input.id}"][data-input-type="multi-select"]`))
            .filter(option => option.checked)
            .map(option => option.dataset.optionValue);
      } else if (input.type === 'list-select') {
        const options = resolveInputOptions(input, info);
        const selected = String(value ?? input.default ?? options[0] ?? '');
        const hidden = document.createElement('input');
        hidden.type = 'hidden';
        hidden.value = selected;
        hidden.dataset.inputId = input.id;
        hidden.dataset.inputType = input.type;
        hidden.dataset.valueKind = inferValueKind(input, value);

        control = document.createElement('div');
        control.className = 'list-select';
        if (options.length === 0) {
          const empty = document.createElement('div');
          empty.className = 'empty';
          empty.textContent = 'No options available right now.';
          control.appendChild(empty);
        } else {
          const updateListUi = () => {
            Array.from(control.querySelectorAll('.list-option')).forEach(option => {
              option.classList.toggle('active', option.dataset.optionValue === hidden.value);
            });
          };

          options.forEach(opt => {
            const option = document.createElement('button');
            option.type = 'button';
            option.className = 'list-option';
            option.dataset.optionValue = opt;
            option.textContent = opt;
            option.addEventListener('click', () => {
              hidden.value = opt;
              updateListUi();
              refreshDirtyState();
            });
            control.appendChild(option);
          });
          updateListUi();
        }
        control.appendChild(hidden);
        readDirtyValue = () => coerceControlValue(hidden, hidden.value);
      } else if (input.type === 'hotkey') {
        const hotkey = createHotkeyInput(input, value);
        control = hotkey.control;
        readDirtyValue = hotkey.readValue;
      } else if (input.type === 'select') {
        control = document.createElement('select');
        resolveInputOptions(input, info).forEach(opt => {
          const o = document.createElement('option');
          o.value = opt;
          o.textContent = (input.optionLabels && input.optionLabels[opt]) || opt;
          control.appendChild(o);
        });
        control.dataset.valueKind = inferValueKind(input, value);
        if (input.id === 'uiTheme') {
          const preferredTheme = value !== undefined && value !== null
            ? value
            : input.default;
          control.value = normalizeThemeId(preferredTheme);
        } else if (value !== undefined && value !== null) {
          control.value = String(value);
        } else if (input.default !== undefined && input.default !== null) {
          control.value = String(input.default);
        }
        readDirtyValue = () => coerceControlValue(control, control.value);
        if (input.id === 'uiTheme') {
          applyTheme(control.value || input.default || 'light');
        }
      } else {
        control = document.createElement('input');
        control.type = (input.type === 'number') ? 'number' : 'text';
        control.value = String(value ?? input.default ?? '');
        readDirtyValue = () =>
          input.type === 'number'
            ? (control.value === '' ? null : Number(control.value))
            : control.value;
      }

      if (input.type !== 'multi-select' && input.type !== 'extension-toggle' && input.type !== 'boolean' && input.type !== 'list-select' && input.type !== 'hotkey') {
        control.dataset.inputId = input.id;
        control.dataset.inputType = input.type;
        if (input.type === 'select') {
          control.dataset.valueKind = control.dataset.valueKind || inferValueKind(input, value);
        }
      }
      if (input.type === 'number') {
        control.dataset.valueKind = 'number';
      }

      if (input.type === 'select' || input.type === 'number' || input.type === 'text' || input.type === 'folder-picker' || input.type === 'file-picker') {
        control.addEventListener('input', () => refreshDirtyState());
        control.addEventListener('change', () => refreshDirtyState());
        if (input.id === 'uiTheme') {
          control.addEventListener('change', () => applyTheme(normalizeThemeId(control.value)));
        }
      }

      wrapper.appendChild(control);
      registerDirtyTracker(wrapper, readDirtyValue || (() => null));
      return wrapper;
    }

    function inferSections(inputs, descriptor) {
      const byInputId = Object.fromEntries((inputs || []).map(input => [input.id, input]));
      const declared = (((descriptor || {}).settings || {}).sections || []);
      const used = new Set();
      const sections = [];

      declared.forEach(section => {
        const inputDefs = (section.inputs || []).map(id => byInputId[id]).filter(Boolean);
        inputDefs.forEach(input => used.add(input.id));
        if (inputDefs.length > 0) {
          sections.push({
            id: section.id,
            title: section.title,
            description: section.description || '',
            inputDefs
          });
        }
      });

      const remaining = (inputs || []).filter(input => !used.has(input.id));
      if (sections.length === 0 && remaining.length > 0) {
        return [{ id: 'settings', title: 'Settings', description: '', inputDefs: remaining }];
      }
      if (remaining.length > 0) {
        sections.push({ id: 'other', title: 'Other settings', description: '', inputDefs: remaining });
      }
      return sections;
    }

