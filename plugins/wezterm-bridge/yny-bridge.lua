local function yny_deep_mux_bridge_dir()
  local runtime_dir = os.getenv('XDG_RUNTIME_DIR')
  if runtime_dir == nil or runtime_dir == '' then
    runtime_dir = '/tmp'
  end
  return runtime_dir .. '/yny-wezterm-mux'
end

local yny_deep_bridge_dir_initialized = false

local function yny_deep_ensure_mux_bridge_dir()
  if yny_deep_bridge_dir_initialized then
    return true
  end

  local success, _, stderr = wezterm.run_child_process {
    'mkdir',
    '-p',
    yny_deep_mux_bridge_dir(),
  }
  if success then
    yny_deep_bridge_dir_initialized = true
    return true
  end

  if stderr ~= nil and stderr ~= '' then
    wezterm.log_warn('yny mux bridge: failed to create bridge dir stderr=' .. stderr)
  end
  return false
end

local function yny_deep_touch_bridge_ready(pane_id)
  if not yny_deep_ensure_mux_bridge_dir() then
    return
  end

  local handle = io.open(string.format('%s/ready', yny_deep_mux_bridge_dir()), 'w')
  if handle == nil then
    return
  end
  handle:write(tostring(pane_id) .. ' ' .. tostring(os.time()) .. '\n')
  handle:close()
end

local function yny_deep_merge_split_flag(dir)
  if dir == 'west' then
    return '--right'
  elseif dir == 'east' then
    return '--left'
  elseif dir == 'north' then
    return '--bottom'
  elseif dir == 'south' then
    return '--top'
  end
  return nil
end

local function yny_deep_bridge_command_path()
  return string.format('%s/merge.cmd', yny_deep_mux_bridge_dir())
end

local function yny_deep_claim_bridge_command(pane_id)
  if not yny_deep_ensure_mux_bridge_dir() then
    return nil, nil
  end
  local cmd_path = yny_deep_bridge_command_path()
  local claimed_path = string.format('%s.processing.%d', cmd_path, pane_id)
  local ok = os.rename(cmd_path, claimed_path)
  if not ok then
    return nil, nil
  end
  return cmd_path, claimed_path
end

local function yny_deep_restore_bridge_command(claimed_path, cmd_path)
  local ok = os.rename(claimed_path, cmd_path)
  if ok then
    return
  end
  wezterm.log_warn('yny mux bridge: failed to restore command file; dropping stale command')
  os.remove(claimed_path)
end

local function yny_deep_process_mux_bridge(window, pane)
  local pane_id = pane:pane_id()
  yny_deep_touch_bridge_ready(pane_id)

  if not window:is_focused() then
    return
  end

  local cmd_path, claimed_path = yny_deep_claim_bridge_command(pane_id)
  if claimed_path == nil then
    return
  end

  local handle = io.open(claimed_path, 'r')
  if handle == nil then
    os.remove(claimed_path)
    return
  end

  local payload = handle:read('*a') or ''
  handle:close()

  local op, source_pane_id_raw, dir = payload:match('^(%S+)%s+(%d+)%s+(%S+)%s*$')
  if op ~= 'merge' then
    wezterm.log_warn('yny mux bridge: unknown command payload=' .. payload)
    os.remove(claimed_path)
    return
  end

  local source_pane_id = tonumber(source_pane_id_raw)
  if source_pane_id == nil then
    wezterm.log_warn('yny mux bridge: invalid source pane id payload=' .. payload)
    os.remove(claimed_path)
    return
  end

  if source_pane_id == pane_id then
    yny_deep_restore_bridge_command(claimed_path, cmd_path)
    return
  end

  local split_flag = yny_deep_merge_split_flag(dir)
  if split_flag == nil then
    wezterm.log_warn('yny mux bridge: invalid direction in payload=' .. payload)
    os.remove(claimed_path)
    return
  end

  local source_pane = mux.get_pane(source_pane_id)
  if source_pane == nil then
    wezterm.log_warn('yny mux bridge: source pane not found id=' .. tostring(source_pane_id))
    os.remove(claimed_path)
    return
  end

  local success, stdout, stderr = wezterm.run_child_process {
    'wezterm',
    'cli',
    'split-pane',
    '--pane-id',
    tostring(pane_id),
    split_flag,
    '--move-pane-id',
    tostring(source_pane_id),
  }

  if not success then
    wezterm.log_error('yny mux bridge: split-pane failed stderr=' .. (stderr or ''))
    yny_deep_restore_bridge_command(claimed_path, cmd_path)
    return
  end

  if stderr ~= nil and stderr ~= '' then
    wezterm.log_warn('yny mux bridge: split-pane stderr=' .. stderr)
  end
  os.remove(claimed_path)

  wezterm.log_info(
    'yny mux bridge: merged source pane '
      .. tostring(source_pane_id)
      .. ' into target pane '
      .. tostring(pane_id)
      .. ' using '
      .. split_flag
      .. (stdout ~= nil and stdout ~= '' and (' stdout=' .. stdout) or '')
  )
end

wezterm.on('update-right-status', yny_deep_process_mux_bridge)