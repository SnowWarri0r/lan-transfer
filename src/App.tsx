import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useState, useEffect } from 'react';

interface Device {
  ip: string;
  hostname: string;
  last_seen: number;
}

interface ReceivedFile {
  name: string;
  size: number;
}

type Mode = 'select' | 'send' | 'receive';
type SendStatus = 'idle' | 'sending' | 'success' | 'error';

function formatSaveDir(dir: string): string {
  if (!dir.startsWith('content://')) return dir;
  try {
    const decoded = decodeURIComponent(dir);
    const treePart = decoded.split('/tree/')[1];
    if (!treePart) return dir;
    const colonIndex = treePart.indexOf(':');
    if (colonIndex === -1) return treePart;
    const storage = treePart.substring(0, colonIndex);
    const path = treePart.substring(colonIndex + 1);
    if (storage === 'primary') {
      return `内部存储/${path}`;
    }
    return `${storage}/${path}`;
  } catch {
    return dir;
  }
}

export default function App() {
  const [mode, setMode] = useState<Mode>('select');
  const [file, setFile] = useState<File | null>(null);
  const [saveDir, setSaveDir] = useState<string | null>(null);
  const [editingSaveDir, setEditingSaveDir] = useState<boolean>(false);
  const [saveDirInput, setSaveDirInput] = useState<string>('');
  const [targetIp, setTargetIp] = useState<string>('');
  const [localIp, setLocalIp] = useState<string>('获取中...');
  const [isReceiving, setIsReceiving] = useState<boolean>(false);
  const [devices, setDevices] = useState<Device[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<Device | null>(null);
  const [sendStatus, setSendStatus] = useState<SendStatus>('idle');
  const [sendingTo, setSendingTo] = useState<string>('');
  const [receivedFiles, setReceivedFiles] = useState<ReceivedFile[]>([]);
  const [receivingFile, setReceivingFile] = useState<string | null>(null);

  // 检测是否为 Android 平台
  const [isAndroid, setIsAndroid] = useState<boolean>(false);

  useEffect(() => {
    // 检测平台
    const checkPlatform = async () => {
      try {
        const platform = await invoke<string>('plugin:os|platform');
        setIsAndroid(platform === 'android');
      } catch {
        // 回退检测方法
        setIsAndroid(navigator.userAgent.toLowerCase().includes('android'));
      }
    };
    checkPlatform();
  }, []);

  useEffect(() => {
    invoke<string>('get_local_ip')
      .then(ip => setLocalIp(ip))
      .catch(err => setLocalIp('获取失败: ' + err));

    invoke('start_discovery');

    const unlistenDevices = listen<Device[]>('devices-updated', (event) => {
      setDevices(event.payload);
    });

    const unlistenReceiving = listen<string>('file-receiving', (event) => {
      setReceivingFile(event.payload);
    });

    const unlistenReceived = listen<ReceivedFile>('file-received', (event) => {
      setReceivingFile(null);
      setReceivedFiles(prev => [event.payload, ...prev].slice(0, 10));
    });

    return () => {
      unlistenDevices.then(fn => fn());
      unlistenReceiving.then(fn => fn());
      unlistenReceived.then(fn => fn());
    };
  }, []);

  useEffect(() => {
    if (mode === 'receive') {
      invoke<string>('get_download_dir')
        .then(dir => {
          setSaveDir(dir);
          invoke('start_websocket_server', { saveDir: dir });
          setIsReceiving(true);
        })
        .catch(() => {
          console.log('无法获取默认下载目录');
        });
    }
  }, [mode]);

  const handlePickFolder = async () => {
    try {
      const selected: string | null = await invoke("select_folder");
      if (selected && selected !== saveDir) {
        setSaveDir(selected);
        if (mode === 'receive') {
          invoke('start_websocket_server', { saveDir: selected });
          setIsReceiving(true);
        }
      }
    } catch (error) {
      console.error('Failed to select folder:', error);
    }
  };

  const handleQuickSelectPath = (path: string) => {
    setSaveDir(path);
    if (mode === 'receive') {
      invoke('start_websocket_server', { saveDir: path });
      setIsReceiving(true);
    }
  };

  const handleSaveDirInputChange = () => {
    if (saveDirInput.trim()) {
      setSaveDir(saveDirInput.trim());
      if (mode === 'receive') {
        invoke('start_websocket_server', { saveDir: saveDirInput.trim() });
        setIsReceiving(true);
      }
      setEditingSaveDir(false);
    }
  };

  const handleSendToDevice = async (device: Device) => {
    if (!file) {
      alert("请先选择文件");
      return;
    }
    setSelectedDevice(device);
    await sendFile(device.ip);
  };

  const handleSendManual = async () => {
    if (!file) {
      alert("请先选择文件");
      return;
    }
    if (!targetIp) {
      alert("请输入目标IP地址");
      return;
    }
    await sendFile(targetIp);
  };

  const sendFile = async (ip: string) => {
    if (!file) return;

    const HIGH_WATER_MARK = 4 * 1024 * 1024; // 4MB buffer threshold

    setSendStatus('sending');
    setSendingTo(ip);

    try {
      const socket = new WebSocket(`ws://${ip}:7878`);
      socket.binaryType = 'arraybuffer';
      let hasError = false;

      socket.onopen = async () => {
        try {
          socket.send(JSON.stringify({ name: file.name }));

          const reader = file.stream().getReader();
          for (;;) {
            const { done, value } = await reader.read();
            if (done) break;

            // Back-pressure: wait if send buffer is congested
            while (socket.bufferedAmount > HIGH_WATER_MARK) {
              await new Promise((r) => setTimeout(r, 50));
            }
            socket.send(value);
          }
          socket.close();
        } catch {
          hasError = true;
          setSendStatus('error');
          setSelectedDevice(null);
        }
      };

      socket.onerror = () => {
        hasError = true;
        setSendStatus('error');
        setSelectedDevice(null);
      };

      socket.onclose = () => {
        if (!hasError) {
          setSendStatus('success');
        }
        setSelectedDevice(null);
      };
    } catch {
      setSendStatus('error');
      setSelectedDevice(null);
    }
  };

  const clearSendStatus = () => {
    setSendStatus('idle');
  };

  // 模式选择界面
  if (mode === 'select') {
    return (
      <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 flex items-center justify-center p-6">
        <div className="w-full max-w-md space-y-6">
          <div className="text-center">
            <h1 className="text-3xl font-bold text-slate-800">局域网文件传输</h1>
            <p className="mt-2 text-slate-500">本机 IP: <span className="font-mono text-slate-700">{localIp}</span></p>
          </div>

          <div className="space-y-4">
            <button
              onClick={() => setMode('send')}
              className="w-full p-6 bg-white border-2 border-slate-200 rounded-xl hover:border-blue-400 hover:shadow-lg transition-all group"
            >
              <div className="text-xl font-semibold text-blue-600 group-hover:text-blue-700">发送模式</div>
              <div className="text-sm text-slate-500 mt-1">选择文件发送给其他设备</div>
            </button>

            <button
              onClick={() => setMode('receive')}
              className="w-full p-6 bg-white border-2 border-slate-200 rounded-xl hover:border-green-400 hover:shadow-lg transition-all group"
            >
              <div className="text-xl font-semibold text-green-600 group-hover:text-green-700">接收模式</div>
              <div className="text-sm text-slate-500 mt-1">监听端口接收其他设备的文件</div>
            </button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 p-6">
      <div className="max-w-2xl mx-auto space-y-6">
        {/* 顶部导航 */}
        <div className="flex items-center justify-between">
          <h1 className="text-2xl font-bold text-slate-800">
            {mode === 'send' ? '发送文件' : '接收文件'}
          </h1>
          <button
            onClick={() => setMode('select')}
            className="px-4 py-2 text-sm text-slate-600 bg-white border border-slate-200 rounded-lg hover:bg-slate-50 transition"
          >
            切换模式
          </button>
        </div>

        {/* 本机信息 */}
        <div className={`p-4 rounded-xl ${mode === 'send' ? 'bg-blue-50 border border-blue-100' : 'bg-green-50 border border-green-100'}`}>
          <p className="text-sm font-medium text-slate-600">本机 IP 地址</p>
          <p className={`text-xl font-mono font-semibold ${mode === 'send' ? 'text-blue-600' : 'text-green-600'}`}>
            {localIp}
          </p>
          {mode === 'receive' && isReceiving && (
            <p className="text-sm text-green-600 mt-1 flex items-center gap-1">
              <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
              正在监听端口 7878
            </p>
          )}
        </div>

        {/* 发送模式 */}
        {mode === 'send' && (
          <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-sm">
            {/* 文件选择 */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-2">选择文件</label>
              <input
                type="file"
                onChange={(e) => setFile(e.target.files?.[0] || null)}
                className="block w-full text-sm text-slate-500 file:mr-4 file:py-2 file:px-4 file:rounded-lg file:border-0 file:text-sm file:font-medium file:bg-blue-50 file:text-blue-700 hover:file:bg-blue-100 transition"
              />
              {file && <p className="text-xs text-slate-500 mt-2">已选择: {file.name}</p>}
            </div>

            {/* 设备列表 */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-2">
                局域网设备 <span className="text-slate-400">({devices.length})</span>
              </label>
              {devices.length > 0 ? (
                <div className="space-y-2 max-h-48 overflow-y-auto">
                  {devices.map((device) => (
                    <div
                      key={device.ip}
                      className={`flex items-center justify-between p-3 border rounded-lg cursor-pointer transition ${
                        selectedDevice?.ip === device.ip
                          ? 'border-blue-400 bg-blue-50'
                          : 'border-slate-200 hover:border-slate-300 hover:bg-slate-50'
                      }`}
                      onClick={() => setSelectedDevice(device)}
                    >
                      <div>
                        <p className="font-medium text-slate-800">{device.hostname}</p>
                        <p className="text-sm text-slate-500 font-mono">{device.ip}</p>
                      </div>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          handleSendToDevice(device);
                        }}
                        disabled={!file || sendStatus === 'sending'}
                        className={`px-4 py-2 text-sm font-medium rounded-lg transition ${
                          file && sendStatus !== 'sending'
                            ? 'bg-blue-500 text-white hover:bg-blue-600'
                            : 'bg-slate-100 text-slate-400 cursor-not-allowed'
                        }`}
                      >
                        发送
                      </button>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="text-center py-8 text-slate-400">
                  <div className="animate-pulse">正在搜索设备...</div>
                </div>
              )}
            </div>

            {/* 手动输入 */}
            <div className="pt-4 border-t border-slate-100">
              <label className="block text-sm font-medium text-slate-700 mb-2">或手动输入 IP 地址</label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={targetIp}
                  onChange={(e) => setTargetIp(e.target.value)}
                  placeholder="例如: 192.168.1.100"
                  className="flex-1 min-w-0 px-4 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                />
                <button
                  onClick={handleSendManual}
                  disabled={sendStatus === 'sending'}
                  className={`px-4 py-2 font-medium rounded-lg transition whitespace-nowrap shrink-0 ${
                    sendStatus === 'sending'
                      ? 'bg-slate-100 text-slate-400 cursor-not-allowed'
                      : 'bg-blue-500 text-white hover:bg-blue-600'
                  }`}
                >
                  {sendStatus === 'sending' ? '发送中...' : '发送'}
                </button>
              </div>
            </div>

            {/* 发送状态 */}
            {sendStatus === 'sending' && (
              <div className="p-4 bg-blue-50 border border-blue-200 rounded-lg flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-5 h-5 border-2 border-blue-500 border-t-transparent rounded-full animate-spin"></div>
                  <span className="text-blue-700">正在发送到 {sendingTo}...</span>
                </div>
              </div>
            )}
            {sendStatus === 'success' && (
              <div className="p-4 bg-green-50 border border-green-200 rounded-lg flex items-center justify-between">
                <span className="text-green-700 font-medium">发送成功！</span>
                <button onClick={clearSendStatus} className="text-green-600 hover:text-green-800 text-xl font-bold">&times;</button>
              </div>
            )}
            {sendStatus === 'error' && (
              <div className="p-4 bg-red-50 border border-red-200 rounded-lg flex items-center justify-between">
                <span className="text-red-700">发送失败，请检查对方是否在线</span>
                <button onClick={clearSendStatus} className="text-red-600 hover:text-red-800 text-xl font-bold">&times;</button>
              </div>
            )}
          </div>
        )}

        {/* 接收模式 */}
        {mode === 'receive' && (
          <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-sm">
            {/* 保存目录 */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-2">保存目录</label>

              {/* 桌面端：显示选择文件夹按钮 */}
              {!isAndroid && (
                <div className="flex flex-wrap gap-2 mb-3">
                  <button
                    onClick={handlePickFolder}
                    className="px-4 py-2 bg-amber-500 text-white rounded-lg hover:bg-amber-600 transition font-medium text-sm"
                  >
                    选择文件夹
                  </button>
                  <button
                    onClick={() => {
                      setEditingSaveDir(true);
                      setSaveDirInput(saveDir || '');
                    }}
                    className="px-4 py-2 bg-slate-500 text-white rounded-lg hover:bg-slate-600 transition font-medium text-sm"
                  >
                    手动输入
                  </button>
                </div>
              )}

              {/* Android：SAF 文件夹选择 + 常用路径快速选择 */}
              {isAndroid && (
                <div className="space-y-2 mb-3">
                  <p className="text-xs text-slate-500">选择保存位置：</p>
                  <div className="grid grid-cols-2 gap-2">
                    <button
                      onClick={handlePickFolder}
                      className="px-3 py-2 rounded-lg border-2 border-amber-300 bg-amber-50 hover:border-amber-500 transition text-left overflow-hidden"
                    >
                      <div className="font-medium text-xs text-amber-800 truncate">📂 选择文件夹</div>
                      <div className="text-xs text-amber-500 mt-0.5 truncate">系统文件选择器</div>
                    </button>
                    <button
                      onClick={() => handleQuickSelectPath('/storage/emulated/0/Download')}
                      className={`px-3 py-2 rounded-lg border-2 transition text-left overflow-hidden ${
                        saveDir === '/storage/emulated/0/Download'
                          ? 'border-green-500 bg-green-50'
                          : 'border-slate-200 bg-white hover:border-green-300'
                      }`}
                    >
                      <div className="font-medium text-xs text-slate-800 truncate">📥 下载目录</div>
                      <div className="text-xs text-slate-400 mt-0.5 truncate">/storage/.../Download</div>
                    </button>
                    <button
                      onClick={() => handleQuickSelectPath('/storage/emulated/0/Documents')}
                      className={`px-3 py-2 rounded-lg border-2 transition text-left overflow-hidden ${
                        saveDir === '/storage/emulated/0/Documents'
                          ? 'border-green-500 bg-green-50'
                          : 'border-slate-200 bg-white hover:border-green-300'
                      }`}
                    >
                      <div className="font-medium text-xs text-slate-800 truncate">📄 文档目录</div>
                      <div className="text-xs text-slate-400 mt-0.5 truncate">/storage/.../Documents</div>
                    </button>
                    <button
                      onClick={() => {
                        setEditingSaveDir(true);
                        setSaveDirInput(saveDir || '');
                      }}
                      className="px-3 py-2 rounded-lg border-2 border-slate-200 bg-white hover:border-amber-300 transition text-left overflow-hidden"
                    >
                      <div className="font-medium text-xs text-slate-800 truncate">✏️ 自定义</div>
                      <div className="text-xs text-slate-400 mt-0.5 truncate">输入路径</div>
                    </button>
                  </div>
                </div>
              )}

              {/* 自定义路径输入 */}
              {editingSaveDir && (
                <div className="space-y-2 p-3 bg-slate-50 rounded-lg border border-slate-200">
                  <input
                    type="text"
                    value={saveDirInput}
                    onChange={(e) => setSaveDirInput(e.target.value)}
                    placeholder="/storage/emulated/0/Download"
                    className="w-full px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-amber-500 focus:border-transparent font-mono text-sm"
                  />
                  <div className="flex gap-2">
                    <button
                      onClick={handleSaveDirInputChange}
                      className="px-4 py-2 bg-green-500 text-white rounded-lg hover:bg-green-600 transition font-medium text-sm"
                    >
                      确认
                    </button>
                    <button
                      onClick={() => setEditingSaveDir(false)}
                      className="px-4 py-2 bg-slate-300 text-slate-700 rounded-lg hover:bg-slate-400 transition font-medium text-sm"
                    >
                      取消
                    </button>
                  </div>
                </div>
              )}

              {/* 当前选中的路径 */}
              {saveDir && !editingSaveDir && (
                <div className="mt-2 p-2 bg-green-50 border border-green-200 rounded-lg">
                  <div className="text-xs text-green-600 mb-1">当前保存位置：</div>
                  <div className="text-sm text-slate-700 font-mono break-all">{formatSaveDir(saveDir)}</div>
                </div>
              )}
            </div>

            {/* 接收状态 */}
            <div className="p-6 bg-slate-50 rounded-lg text-center">
              {receivingFile ? (
                <div className="text-blue-600 font-medium flex items-center justify-center gap-2">
                  <div className="w-4 h-4 border-2 border-blue-500 border-t-transparent rounded-full animate-spin"></div>
                  正在接收: {receivingFile}
                </div>
              ) : isReceiving ? (
                <>
                  <div className="text-green-600 font-semibold text-lg flex items-center justify-center gap-2">
                    <span className="w-3 h-3 bg-green-500 rounded-full animate-pulse"></span>
                    等待接收文件...
                  </div>
                  <div className="text-sm text-slate-500 mt-2">
                    其他设备可以发送文件到 <span className="font-mono text-slate-700">{localIp}:7878</span>
                  </div>
                </>
              ) : (
                <div className="text-slate-500">正在启动接收服务...</div>
              )}
            </div>

            {/* 已接收文件 */}
            {receivedFiles.length > 0 && (
              <div className="pt-4 border-t border-slate-100">
                <label className="block text-sm font-medium text-slate-700 mb-2">已接收的文件</label>
                <div className="space-y-2 max-h-40 overflow-y-auto">
                  {receivedFiles.map((f, i) => (
                    <div key={i} className="p-3 bg-green-50 border border-green-100 rounded-lg flex items-center justify-between">
                      <span className="text-green-700 font-medium">{f.name}</span>
                      <span className="text-slate-500 text-sm">{(f.size / 1024).toFixed(1)} KB</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {/* 发现的设备 */}
            {devices.length > 0 && (
              <div className="pt-4 border-t border-slate-100">
                <label className="block text-sm font-medium text-slate-700 mb-2">
                  发现的其他设备 <span className="text-slate-400">({devices.length})</span>
                </label>
                <div className="space-y-1">
                  {devices.map((device) => (
                    <div key={device.ip} className="text-sm text-slate-600 py-1">
                      {device.hostname} <span className="font-mono text-slate-400">({device.ip})</span>
                    </div>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
