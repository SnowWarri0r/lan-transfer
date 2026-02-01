import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useState, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';

interface Device {
  ip: string;
  hostname: string;
  last_seen: number;
}

interface ReceivedFile {
  name: string;
  size: number;
}

interface ChatMessage {
  content: string;
  from_ip: string;
  timestamp: number;
  is_me?: boolean;
}

interface FileQueueItem {
  file: File;
  id: string;
  status: 'pending' | 'sending' | 'completed' | 'failed';
  progress: number;
  bytesTransferred: number;
  speed: number;
  error?: string;
}

type Mode = 'select' | 'send' | 'receive' | 'chat';
type SendStatus = 'idle' | 'sending' | 'success' | 'error';

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(2)} MB/s`;
}

function formatSaveDir(dir: string, t: (key: string) => string): string {
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
      return `${t('receive.android.internalStorage')}${path}`;
    }
    return `${storage}/${path}`;
  } catch {
    return dir;
  }
}

export default function App() {
  const { t, i18n } = useTranslation();
  const [mode, setMode] = useState<Mode>('select');
  const [fileQueue, setFileQueue] = useState<FileQueueItem[]>([]);
  const [currentFileIndex, setCurrentFileIndex] = useState<number>(-1);
  const [saveDir, setSaveDir] = useState<string | null>(null);
  const [editingSaveDir, setEditingSaveDir] = useState<boolean>(false);
  const [saveDirInput, setSaveDirInput] = useState<string>('');
  const [targetIp, setTargetIp] = useState<string>('');
  const [localIp, setLocalIp] = useState<string>(t('common.loading'));
  const [isReceiving, setIsReceiving] = useState<boolean>(false);
  const [devices, setDevices] = useState<Device[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<Device | null>(null);
  const [sendStatus, setSendStatus] = useState<SendStatus>('idle');
  const [sendingTo, setSendingTo] = useState<string>('');
  const [receivedFiles, setReceivedFiles] = useState<ReceivedFile[]>([]);
  const [receivingFile, setReceivingFile] = useState<string | null>(null);
  const [receiveCancelledFile, setReceiveCancelledFile] = useState<string | null>(null);
  const [receivingProgress, setReceivingProgress] = useState<{
    fileName: string;
    progress: number;
    received: number;
    total: number;
  } | null>(null);
  const [sendingProgress, setSendingProgress] = useState<{
    fileName: string;
    progress: number;
    sent: number;
    total: number;
  } | null>(null);

  // 取消发送标志
  const cancelSendingRef = useRef<boolean>(false);

  // 聊天模式状态
  const [chatMessages, setChatMessages] = useState<ChatMessage[]>([]);
  const [chatInput, setChatInput] = useState<string>('');
  const [activeChatIp, setActiveChatIp] = useState<string | null>(null);
  const [chatConnected, setChatConnected] = useState<boolean>(false);
  const [chatError, setChatError] = useState<string | null>(null);
  const [selectedMessageIndex, setSelectedMessageIndex] = useState<number | null>(null);
  const [copiedMessageIndex, setCopiedMessageIndex] = useState<number | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const chatConnectedRef = useRef<boolean>(false);
  const activeChatIpRef = useRef<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // 检测是否为 Android 平台
  const [isAndroid, setIsAndroid] = useState<boolean>(false);

  // 根据语言设置窗口标题
  useEffect(() => {
    getCurrentWindow().setTitle(t('mode.title')).catch(() => {});
  }, [i18n.language]);

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
      .catch(err => setLocalIp(t('error.fetchIpFailed') + err));

    invoke('start_discovery');

    const unlistenDevices = listen<Device[]>('devices-updated', (event) => {
      setDevices(event.payload);
    });

    const unlistenReceiving = listen<string>('file-receiving', (event) => {
      setReceivingFile(event.payload);
    });

    const unlistenReceived = listen<ReceivedFile>('file-received', (event) => {
      setReceivingFile(null);
      setReceivingProgress(null);
      setReceivedFiles(prev => [event.payload, ...prev].slice(0, 10));
    });

    const unlistenCancelled = listen<string>('file-receive-cancelled', (event) => {
      console.log('File receive cancelled:', event.payload);
      setReceivingFile(null);
      setReceivingProgress(null);
      setReceiveCancelledFile(event.payload);
      setTimeout(() => setReceiveCancelledFile(null), 3000);
    });

    const unlistenProgress = listen<{
      file_name: string;
      bytes_received: number;
      total_bytes: number;
      percentage: number;
    }>('file-transfer-progress', (event) => {
      // 接收进度
      setReceivingProgress({
        fileName: event.payload.file_name,
        progress: event.payload.percentage,
        received: event.payload.bytes_received,
        total: event.payload.total_bytes,
      });
      // 发送进度（Android 发送时也会触发此事件）
      setSendingProgress({
        fileName: event.payload.file_name,
        progress: event.payload.percentage,
        sent: event.payload.bytes_received,
        total: event.payload.total_bytes,
      });
    });

    return () => {
      unlistenDevices.then(fn => fn());
      unlistenReceiving.then(fn => fn());
      unlistenReceived.then(fn => fn());
      unlistenCancelled.then(fn => fn());
      unlistenProgress.then(fn => fn());
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
          console.log(t('error.downloadDirFailed'));
        });
    } else if (mode === 'chat') {
      invoke('start_chat_server');
    }
  }, [mode]);

  // 聊天事件监听
  useEffect(() => {
    const unlistenMessage = listen<ChatMessage>('chat-message-received', (event) => {
      const msg = event.payload;
      setChatMessages(prev => [...prev, { ...msg, is_me: false }]);
    });

    const unlistenConnected = listen<string>('chat-connected', (event) => {
      const peerIp = event.payload;

      // 情况1：如果对方是当前聊天对象
      if (peerIp === activeChatIpRef.current) {
        // 如果当前未连接，说明对方重新连接了，我们也需要重新连接以建立双向通道
        if (!chatConnectedRef.current) {
          // 延迟一点以确保对方服务器已准备好
          setTimeout(async () => {
            try {
              await invoke('connect_to_chat', { targetIp: peerIp });
            } catch (err) {
              console.error('Failed to reconnect:', err);
            }
          }, 300);
        }
        setChatConnected(true);
        chatConnectedRef.current = true;
        setChatError(null);
      }
      // 情况2：如果我们还在设备选择界面，对方主动连接了我们
      else if (!activeChatIpRef.current) {
        // 自动接受连接，进入聊天界面并建立反向连接
        setActiveChatIp(peerIp);
        activeChatIpRef.current = peerIp;
        setChatMessages([]);
        // 延迟一点以确保对方服务器已准备好
        setTimeout(async () => {
          try {
            await invoke('connect_to_chat', { targetIp: peerIp });
          } catch (err) {
            console.error('Failed to establish reverse connection:', err);
          }
        }, 300);
        setChatConnected(true);
        chatConnectedRef.current = true;
        setChatError(null);
      }
    });

    const unlistenDisconnected = listen<string>('chat-disconnected', (event) => {
      const peerIp = event.payload;
      if (peerIp === activeChatIpRef.current) {
        setChatConnected(false);
        chatConnectedRef.current = false;
        setChatError(t('chat.connectionLost'));
      }
    });

    const unlistenError = listen<string>('chat-server-error', (event) => {
      setChatError(event.payload);
    });

    return () => {
      unlistenMessage.then(fn => fn());
      unlistenConnected.then(fn => fn());
      unlistenDisconnected.then(fn => fn());
      unlistenError.then(fn => fn());
    };
  }, []);

  // 自动滚动到最新消息
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [chatMessages]);

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
    if (fileQueue.length === 0 && !isAndroid) {
      alert(t('send.selectFileFirst'));
      return;
    }
    setSelectedDevice(device);

    // Android: 使用原生选择器并直接发送
    if (isAndroid) {
      await handleAndroidSend(device.ip);
    } else {
      await sendFiles(device.ip);
    }
  };

  const handleSendManual = async () => {
    if (fileQueue.length === 0 && !isAndroid) {
      alert(t('send.selectFileFirst'));
      return;
    }
    if (!targetIp) {
      alert(t('send.enterIp'));
      return;
    }

    // Android: 使用原生选择器并直接发送
    if (isAndroid) {
      await handleAndroidSend(targetIp);
    } else {
      await sendFiles(targetIp);
    }
  };

  const handleAndroidSend = async (ip: string) => {
    try {
      setSendStatus('sending');
      setSendingTo(ip);
      setSendingProgress(null);

      // 调用 Android 原生多选
      const uris: string[] = await invoke('pick_multiple_files');

      if (!uris || uris.length === 0) {
        setSendStatus('idle');
        return;
      }

      // 使用 Rust 发送文件
      await invoke('send_files_android', { uris, targetIp: ip });

      setSendStatus('success');
      setSendingProgress(null);
      setSelectedDevice(null);
    } catch (error) {
      console.error('Android send failed:', error);
      const errorMsg = String(error);

      if (errorMsg.includes('Cancelled by user')) {
        setSendStatus('idle');
      } else if (errorMsg.includes('Cancelled by receiver') || errorMsg.includes('Broken pipe') || errorMsg.includes('Connection reset')) {
        setSendStatus('idle');
        alert(t('send.cancelledByReceiver'));
      } else {
        setSendStatus('error');
        alert(t('send.failed') + ': ' + error);
      }

      setSendingProgress(null);
      setSelectedDevice(null);
    }
  };

  const updateItemStatus = (
    index: number,
    status: FileQueueItem['status'],
    error?: string
  ) => {
    setFileQueue(prev => prev.map((item, i) =>
      i === index ? { ...item, status, error } : item
    ));
  };

  const updateFileProgress = (
    index: number,
    update: { progress: number; bytesTransferred: number; speed: number }
  ) => {
    setFileQueue(prev => prev.map((item, i) =>
      i === index ? { ...item, ...update } : item
    ));
  };

  const sendSingleFile = async (
    file: File,
    ip: string,
    index: number,
    total: number,
    queueIndex: number
  ): Promise<void> => {
    const HIGH_WATER_MARK = 4 * 1024 * 1024;

    return new Promise((resolve, reject) => {
      const socket = new WebSocket(`ws://${ip}:7878`);
      socket.binaryType = 'arraybuffer';
      let hasError = false;

      let bytesSent = 0;
      let startTime = Date.now();
      let lastUpdateTime = startTime;
      let lastBytesSent = 0;

      socket.onopen = async () => {
        try {
          // Send metadata with new fields
          socket.send(JSON.stringify({
            name: file.name,
            size: file.size,
            index,
            total
          }));

          // Stream file data
          const reader = file.stream().getReader();

          for (;;) {
            // Check if cancelled
            if (cancelSendingRef.current) {
              reader.cancel();
              socket.close();
              hasError = true;
              reject(new Error('Cancelled by user'));
              return;
            }

            const { done, value } = await reader.read();
            if (done) break;

            // Back-pressure control
            while (socket.bufferedAmount > HIGH_WATER_MARK) {
              await new Promise(r => setTimeout(r, 50));
              // Check cancel during back-pressure wait
              if (cancelSendingRef.current) {
                reader.cancel();
                socket.close();
                hasError = true;
                reject(new Error('Cancelled by user'));
                return;
              }
            }

            socket.send(value);
            bytesSent += value.byteLength;

            // Update progress every 100ms
            const now = Date.now();
            if (now - lastUpdateTime > 100) {
              const timeDelta = (now - lastUpdateTime) / 1000;
              const bytesDelta = bytesSent - lastBytesSent;
              const speed = bytesDelta / timeDelta;

              updateFileProgress(queueIndex, {
                progress: (bytesSent / file.size) * 100,
                bytesTransferred: bytesSent,
                speed
              });

              lastUpdateTime = now;
              lastBytesSent = bytesSent;
            }
          }

          socket.close();
        } catch (err) {
          hasError = true;
          reject(err);
        }
      };

      socket.onerror = () => {
        hasError = true;
        reject(new Error('Connection failed'));
      };

      socket.onclose = (event) => {
        if (!hasError) {
          if (event.code === 4001) {
            hasError = true;
            reject(new Error('Cancelled by receiver'));
          } else {
            resolve();
          }
        }
      };
    });
  };

  const sendFiles = async (ip: string) => {
    if (fileQueue.length === 0) return;

    cancelSendingRef.current = false; // Reset cancel flag
    setSendStatus('sending');
    setSendingTo(ip);
    const totalFiles = fileQueue.length;
    let allSucceeded = true;

    for (let i = 0; i < fileQueue.length; i++) {
      // Check if cancelled before starting next file
      if (cancelSendingRef.current) {
        updateItemStatus(i, 'failed', 'Cancelled');
        allSucceeded = false;
        break;
      }

      setCurrentFileIndex(i);
      updateItemStatus(i, 'sending');

      try {
        await sendSingleFile(fileQueue[i].file, ip, i, totalFiles, i);
        updateItemStatus(i, 'completed');
      } catch (error) {
        const errMsg = (error as Error).message;
        const cancelledByReceiver = errMsg.includes('Cancelled by receiver');
        updateItemStatus(i, 'failed', cancelledByReceiver ? t('send.cancelledByReceiver') : errMsg);
        allSucceeded = false;
        if (cancelledByReceiver) {
          alert(t('send.cancelledByReceiver'));
        }
        break; // Stop on error
      }
    }

    const wasCancelled = cancelSendingRef.current;
    setSendStatus(wasCancelled ? 'idle' : (allSucceeded ? 'success' : 'error'));
    setCurrentFileIndex(-1);
    setSelectedDevice(null);
    cancelSendingRef.current = false; // Reset after done
  };

  const clearSendStatus = () => {
    setSendStatus('idle');
  };

  const cancelSending = async () => {
    cancelSendingRef.current = true;
    setSendingProgress(null);

    // 同时取消Android端的发送（如果正在进行）
    try {
      await invoke('cancel_file_sending');
    } catch (error) {
      // Ignore error if command fails (e.g., on non-Android platforms)
    }
  };

  const removeFileFromQueue = (id: string) => {
    setFileQueue(prev => prev.filter(item => item.id !== id));
  };

  // 聊天处理函数
  const handleStartChat = async (device: Device) => {
    setChatError(null);
    try {
      await invoke('connect_to_chat', { targetIp: device.ip });
      setActiveChatIp(device.ip);
      activeChatIpRef.current = device.ip;
      setChatMessages([]);
      setChatConnected(true);
      chatConnectedRef.current = true;
      setChatError(null);
    } catch (err) {
      setChatError(t('chat.connectionFailed') + err);
      setActiveChatIp(null);
      activeChatIpRef.current = null;
    }
  };

  const handleSendChatMessage = async () => {
    if (!chatInput.trim() || !activeChatIp) return;

    try {
      await invoke('send_chat_message', { targetIp: activeChatIp, content: chatInput });

      const timestamp = Date.now();
      setChatMessages(prev => [...prev, {
        content: chatInput,
        from_ip: localIp,
        timestamp,
        is_me: true
      }]);

      setChatInput('');
    } catch (err) {
      alert(t('chat.sendFailed') + err);
    }
  };

  const handleDisconnectChat = async () => {
    if (activeChatIp) {
      try {
        await invoke('disconnect_chat', { targetIp: activeChatIp });
      } catch (err) {
        console.error(t('error.disconnectFailed'), err);
      }
    }
    setActiveChatIp(null);
    activeChatIpRef.current = null;
    setChatMessages([]);
    setChatConnected(false);
    chatConnectedRef.current = false;
  };

  const handleLeaveChatMode = async () => {
    try {
      await invoke('disconnect_all_chats');
      await invoke('stop_chat_server');
    } catch (err) {
      console.error(t('error.stopChatFailed'), err);
    }
    setActiveChatIp(null);
    activeChatIpRef.current = null;
    setChatMessages([]);
    setChatConnected(false);
    chatConnectedRef.current = false;
  };

  const formatTime = (timestamp: number) => {
    const date = new Date(timestamp);
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' });
  };

  const handleCopyMessage = async (content: string, index: number) => {
    try {
      await navigator.clipboard.writeText(content);
      setCopiedMessageIndex(index);
      // 短暂显示"已复制"后自动隐藏
      setTimeout(() => {
        setCopiedMessageIndex(null);
        setSelectedMessageIndex(null);
      }, 1500);
    } catch (err) {
      console.error(t('error.copyFailed'), err);
    }
  };

  // 模式选择界面
  if (mode === 'select') {
    return (
      <div className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 flex items-center justify-center p-6">
        <div className="w-full max-w-md space-y-6">
          <div className="text-center">
            <h1 className="text-3xl font-bold text-slate-800">{t('mode.title')}</h1>
            <p className="mt-2 text-slate-500">{t('common.localIp')}: <span className="font-mono text-slate-700">{localIp}</span></p>
          </div>

          <div className="space-y-4">
            <button
              onClick={() => setMode('send')}
              className="w-full p-6 bg-white border-2 border-slate-200 rounded-xl hover:border-blue-400 hover:shadow-lg transition-all group"
            >
              <div className="text-xl font-semibold text-blue-600 group-hover:text-blue-700">{t('mode.send')}</div>
              <div className="text-sm text-slate-500 mt-1">{t('mode.sendDesc')}</div>
            </button>

            <button
              onClick={() => setMode('receive')}
              className="w-full p-6 bg-white border-2 border-slate-200 rounded-xl hover:border-green-400 hover:shadow-lg transition-all group"
            >
              <div className="text-xl font-semibold text-green-600 group-hover:text-green-700">{t('mode.receive')}</div>
              <div className="text-sm text-slate-500 mt-1">{t('mode.receiveDesc')}</div>
            </button>

            <button
              onClick={() => setMode('chat')}
              className="w-full p-6 bg-white border-2 border-slate-200 rounded-xl hover:border-purple-400 hover:shadow-lg transition-all group"
            >
              <div className="text-xl font-semibold text-purple-600 group-hover:text-purple-700">{t('mode.chat')}</div>
              <div className="text-sm text-slate-500 mt-1">{t('mode.chatDesc')}</div>
            </button>
          </div>

          <div className="absolute top-4 right-4">
            <button
              onClick={() => i18n.changeLanguage(i18n.language === 'zh' ? 'en' : 'zh')}
              className="px-4 py-2 bg-white/10 hover:bg-white/20 rounded-lg transition-colors"
            >
              {i18n.language === 'zh' ? 'EN' : '中文'}
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
            {mode === 'send' ? t('send.title') : mode === 'receive' ? t('receive.title') : t('chat.title')}
          </h1>
          <button
            onClick={async () => {
              if (mode === 'chat') {
                await handleLeaveChatMode();
              }
              setMode('select');
            }}
            className="px-4 py-2 text-sm text-slate-600 bg-white border border-slate-200 rounded-lg hover:bg-slate-50 transition"
          >
            {t('mode.switchMode')}
          </button>
        </div>

        {/* 本机信息 */}
        {mode !== 'chat' && (
          <div className={`p-4 rounded-xl ${mode === 'send' ? 'bg-blue-50 border border-blue-100' : 'bg-green-50 border border-green-100'}`}>
            <p className="text-sm font-medium text-slate-600">{t('common.localIp')}</p>
            <p className={`text-xl font-mono font-semibold ${mode === 'send' ? 'text-blue-600' : 'text-green-600'}`}>
              {localIp}
            </p>
            {mode === 'receive' && isReceiving && (
              <p className="text-sm text-green-600 mt-1 flex items-center gap-1">
                <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
                {t('receive.listening')}
              </p>
            )}
          </div>
        )}

        {/* 发送模式 */}
        {mode === 'send' && (
          <div className="bg-white rounded-xl border border-slate-200 p-5 space-y-4 shadow-sm">
            {/* 文件选择 */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-2">{t('send.selectFile')}</label>

              {/* 桌面端：支持多选和拖拽 */}
              {!isAndroid && (
                <>
                  <input
                    ref={fileInputRef}
                    type="file"
                    multiple
                    onChange={(e) => {
                      const files = Array.from(e.target.files || []);
                      if (files.length > 0) {
                        const items: FileQueueItem[] = files.map((file, index) => ({
                          file,
                          id: `${Date.now()}-${index}`,
                          status: 'pending',
                          progress: 0,
                          bytesTransferred: 0,
                          speed: 0,
                        }));
                        setFileQueue(items);
                      }
                      // Reset input to allow selecting same files again
                      e.target.value = '';
                    }}
                    className="hidden"
                  />
                  <div
                    onDrop={(e) => {
                      e.preventDefault();
                      e.currentTarget.classList.remove('border-blue-400', 'bg-blue-50');
                      const files = Array.from(e.dataTransfer.files);
                      if (files.length > 0) {
                        const items: FileQueueItem[] = files.map((file, index) => ({
                          file,
                          id: `${Date.now()}-${index}`,
                          status: 'pending',
                          progress: 0,
                          bytesTransferred: 0,
                          speed: 0,
                        }));
                        setFileQueue(items);
                      }
                    }}
                    onDragOver={(e) => {
                      e.preventDefault();
                      e.currentTarget.classList.add('border-blue-400', 'bg-blue-100');
                    }}
                    onDragLeave={(e) => {
                      e.currentTarget.classList.remove('border-blue-400', 'bg-blue-100');
                    }}
                    onClick={() => fileInputRef.current?.click()}
                    className="border-2 border-dashed border-slate-300 rounded-xl p-8 text-center transition-colors cursor-pointer hover:border-blue-300 hover:bg-blue-50"
                  >
                    <div className="text-4xl mb-2">📁</div>
                    <p className="font-medium text-slate-700">{t('send.dragDropHint')}</p>
                    <p className="text-sm text-slate-500 mt-1">{t('send.orClickToSelect')}</p>
                  </div>
                </>
              )}

              {/* Android端：简洁提示 */}
              {isAndroid && (
                <div className="p-3 bg-blue-50 border border-blue-200 rounded-lg">
                  <p className="text-sm text-slate-600 text-center">
                    📱 {t('send.androidHintShort')}
                  </p>
                </div>
              )}

              {/* 文件队列 */}
              {fileQueue.length > 0 && (
                <div className="space-y-2 max-h-80 overflow-y-auto mt-3">
                  {fileQueue.map((item) => (
                    <div key={item.id} className="border rounded-lg p-3 bg-white">
                      <div className="flex justify-between items-center">
                        <div className="flex-1 min-w-0">
                          <p className="font-medium text-sm truncate">{item.file.name}</p>
                          <p className="text-xs text-slate-500">
                            {formatBytes(item.file.size)}
                            {item.status === 'sending' && item.speed > 0 &&
                              ` - ${formatSpeed(item.speed)}`
                            }
                          </p>
                        </div>

                        {/* 状态图标和删除按钮 */}
                        <div className="ml-3 flex items-center gap-2">
                          {item.status === 'pending' && (
                            <>
                              <span className="text-slate-400">⏳</span>
                              <button
                                onClick={() => removeFileFromQueue(item.id)}
                                className="text-red-500 hover:text-red-700 text-lg"
                                title={t('send.removeFile')}
                              >
                                🗑️
                              </button>
                            </>
                          )}
                          {item.status === 'sending' && <span className="text-blue-500">📤</span>}
                          {item.status === 'completed' && <span className="text-green-500">✅</span>}
                          {item.status === 'failed' && <span className="text-red-500">❌</span>}
                        </div>
                      </div>

                      {/* 进度条 */}
                      {item.status === 'sending' && (
                        <div className="mt-2">
                          <div className="w-full bg-slate-200 rounded-full h-2 overflow-hidden">
                            <div
                              className="bg-blue-500 h-2 rounded-full transition-all duration-300"
                              style={{ width: `${item.progress}%` }}
                            />
                          </div>
                          <p className="text-xs text-slate-600 mt-1">{item.progress.toFixed(1)}%</p>
                        </div>
                      )}

                      {item.error && (
                        <p className="text-xs text-red-500 mt-1">{item.error}</p>
                      )}
                    </div>
                  ))}
                </div>
              )}

              {/* 整体进度（含取消按钮） */}
              {currentFileIndex >= 0 && (
                <div className="p-4 bg-blue-50 rounded-lg mt-3 border border-blue-200">
                  <div className="flex items-start justify-between gap-2 mb-2">
                    <div className="flex-1 min-w-0">
                      <p className="text-sm font-medium text-blue-700">
                        {t('send.sendingFile')} {currentFileIndex + 1} / {fileQueue.length}
                      </p>
                      <p className="text-xs text-blue-500 mt-0.5">{t('send.sendingTo')}{sendingTo}</p>
                    </div>
                    <button
                      onClick={cancelSending}
                      className="px-3 py-1.5 bg-red-500 text-white text-xs rounded hover:bg-red-600 transition-colors flex-shrink-0 whitespace-nowrap"
                    >
                      {t('common.cancel')}
                    </button>
                  </div>
                  <div className="w-full bg-blue-200 rounded-full h-3 overflow-hidden">
                    <div
                      className="bg-blue-600 h-3 rounded-full transition-all duration-300"
                      style={{ width: `${((currentFileIndex + 1) / fileQueue.length) * 100}%` }}
                    />
                  </div>
                </div>
              )}
            </div>

            {/* 发送进度条（Android/桌面通用） */}
            {sendingProgress && sendStatus === 'sending' && (
              <div className="p-4 bg-green-50 rounded-lg border border-green-200">
                <div className="flex items-start justify-between gap-2 mb-2">
                  <div className="flex items-center gap-2 flex-1 min-w-0">
                    <div className="w-4 h-4 border-2 border-green-500 border-t-transparent rounded-full animate-spin flex-shrink-0"></div>
                    <span className="font-medium text-green-700 text-sm truncate">{t('send.sending')}: {sendingProgress.fileName}</span>
                  </div>
                  <button
                    onClick={cancelSending}
                    className="px-3 py-1.5 bg-red-500 text-white text-xs rounded hover:bg-red-600 transition-colors flex-shrink-0 whitespace-nowrap"
                  >
                    {t('common.cancel')}
                  </button>
                </div>

                <div className="w-full bg-green-200 rounded-full h-3 overflow-hidden">
                  <div
                    className="bg-green-600 h-3 rounded-full transition-all duration-300"
                    style={{ width: `${sendingProgress.progress}%` }}
                  />
                </div>

                <div className="flex justify-between text-xs text-slate-600 mt-1">
                  <span>{sendingProgress.progress.toFixed(1)}%</span>
                  <span>{formatBytes(sendingProgress.sent)} / {formatBytes(sendingProgress.total)}</span>
                </div>
              </div>
            )}

            {/* 设备列表 */}
            <div>
              <label className="block text-sm font-medium text-slate-700 mb-2">
                {t('send.devices')} <span className="text-slate-400">({devices.length})</span>
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
                        disabled={(!isAndroid && fileQueue.length === 0) || sendStatus === 'sending'}
                        className={`px-4 py-2 text-sm font-medium rounded-lg transition ${
                          (isAndroid || fileQueue.length > 0) && sendStatus !== 'sending'
                            ? 'bg-blue-500 text-white hover:bg-blue-600'
                            : 'bg-slate-100 text-slate-400 cursor-not-allowed'
                        }`}
                      >
                        {t('common.send')}
                      </button>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="text-center py-8 text-slate-400">
                  <div className="animate-pulse">{t('chat.searching')}</div>
                </div>
              )}
            </div>

            {/* 手动输入 */}
            <div className="pt-4 border-t border-slate-100">
              <label className="block text-sm font-medium text-slate-700 mb-2">{t('send.manualIp')}</label>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={targetIp}
                  onChange={(e) => setTargetIp(e.target.value)}
                  placeholder={t('send.ipPlaceholder')}
                  className="flex-1 min-w-0 px-4 py-2 border border-slate-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                />
                <button
                  onClick={handleSendManual}
                  disabled={(!isAndroid && fileQueue.length === 0) || sendStatus === 'sending'}
                  className={`px-4 py-2 font-medium rounded-lg transition whitespace-nowrap shrink-0 ${
                    (!isAndroid && fileQueue.length === 0) || sendStatus === 'sending'
                      ? 'bg-slate-100 text-slate-400 cursor-not-allowed'
                      : 'bg-blue-500 text-white hover:bg-blue-600'
                  }`}
                >
                  {sendStatus === 'sending' ? t('send.sending') : t('common.send')}
                </button>
              </div>
            </div>

            {/* 发送状态（仅 Android 或队列未启动时显示，桌面端由整体进度栏覆盖） */}
            {sendStatus === 'sending' && currentFileIndex < 0 && (
              <div className="p-4 bg-blue-50 border border-blue-200 rounded-lg flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-5 h-5 border-2 border-blue-500 border-t-transparent rounded-full animate-spin"></div>
                  <span className="text-blue-700">{t('send.sendingTo')}{sendingTo}...</span>
                </div>
              </div>
            )}
            {sendStatus === 'success' && (
              <div className="p-4 bg-green-50 border border-green-200 rounded-lg flex items-center justify-between">
                <span className="text-green-700 font-medium">{t('send.success')}</span>
                <button onClick={clearSendStatus} className="text-green-600 hover:text-green-800 text-xl font-bold">&times;</button>
              </div>
            )}
            {sendStatus === 'error' && (
              <div className="p-4 bg-red-50 border border-red-200 rounded-lg flex items-center justify-between">
                <span className="text-red-700">{t('send.failed')}</span>
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
              <label className="block text-sm font-medium text-slate-700 mb-2">{t('receive.saveDir')}</label>

              {/* 桌面端：显示选择文件夹按钮 */}
              {!isAndroid && (
                <div className="flex flex-wrap gap-2 mb-3">
                  <button
                    onClick={handlePickFolder}
                    className="px-4 py-2 bg-amber-500 text-white rounded-lg hover:bg-amber-600 transition font-medium text-sm"
                  >
                    {t('receive.selectFolder')}
                  </button>
                  <button
                    onClick={() => {
                      setEditingSaveDir(true);
                      setSaveDirInput(saveDir || '');
                    }}
                    className="px-4 py-2 bg-slate-500 text-white rounded-lg hover:bg-slate-600 transition font-medium text-sm"
                  >
                    {t('receive.manualInput')}
                  </button>
                </div>
              )}

              {/* Android：SAF 文件夹选择 + 常用路径快速选择 */}
              {isAndroid && (
                <div className="space-y-2 mb-3">
                  <p className="text-xs text-slate-500">{t('receive.android.selectLocation')}</p>
                  <div className="grid grid-cols-2 gap-2">
                    <button
                      onClick={handlePickFolder}
                      className="px-3 py-2 rounded-lg border-2 border-amber-300 bg-amber-50 hover:border-amber-500 transition text-left overflow-hidden"
                    >
                      <div className="font-medium text-xs text-amber-800 truncate">{t('receive.android.picker')}</div>
                      <div className="text-xs text-amber-500 mt-0.5 truncate">{t('receive.android.pickerDesc')}</div>
                    </button>
                    <button
                      onClick={() => handleQuickSelectPath('/storage/emulated/0/Download')}
                      className={`px-3 py-2 rounded-lg border-2 transition text-left overflow-hidden ${
                        saveDir === '/storage/emulated/0/Download'
                          ? 'border-green-500 bg-green-50'
                          : 'border-slate-200 bg-white hover:border-green-300'
                      }`}
                    >
                      <div className="font-medium text-xs text-slate-800 truncate">{t('receive.android.downloads')}</div>
                      <div className="text-xs text-slate-400 mt-0.5 truncate">{t('receive.android.downloadsPath')}</div>
                    </button>
                    <button
                      onClick={() => handleQuickSelectPath('/storage/emulated/0/Documents')}
                      className={`px-3 py-2 rounded-lg border-2 transition text-left overflow-hidden ${
                        saveDir === '/storage/emulated/0/Documents'
                          ? 'border-green-500 bg-green-50'
                          : 'border-slate-200 bg-white hover:border-green-300'
                      }`}
                    >
                      <div className="font-medium text-xs text-slate-800 truncate">{t('receive.android.documents')}</div>
                      <div className="text-xs text-slate-400 mt-0.5 truncate">{t('receive.android.documentsPath')}</div>
                    </button>
                    <button
                      onClick={() => {
                        setEditingSaveDir(true);
                        setSaveDirInput(saveDir || '');
                      }}
                      className="px-3 py-2 rounded-lg border-2 border-slate-200 bg-white hover:border-amber-300 transition text-left overflow-hidden"
                    >
                      <div className="font-medium text-xs text-slate-800 truncate">{t('receive.android.custom')}</div>
                      <div className="text-xs text-slate-400 mt-0.5 truncate">{t('receive.android.customDesc')}</div>
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
                    placeholder={t('receive.android.customPlaceholder')}
                    className="w-full px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-amber-500 focus:border-transparent font-mono text-sm"
                  />
                  <div className="flex gap-2">
                    <button
                      onClick={handleSaveDirInputChange}
                      className="px-4 py-2 bg-green-500 text-white rounded-lg hover:bg-green-600 transition font-medium text-sm"
                    >
                      {t('common.confirm')}
                    </button>
                    <button
                      onClick={() => setEditingSaveDir(false)}
                      className="px-4 py-2 bg-slate-300 text-slate-700 rounded-lg hover:bg-slate-400 transition font-medium text-sm"
                    >
                      {t('common.cancel')}
                    </button>
                  </div>
                </div>
              )}

              {/* 当前选中的路径 */}
              {saveDir && !editingSaveDir && (
                <div className="mt-2 p-2 bg-green-50 border border-green-200 rounded-lg">
                  <div className="text-xs text-green-600 mb-1">{t('receive.currentPath')}</div>
                  <div className="text-sm text-slate-700 font-mono break-all">{formatSaveDir(saveDir, t)}</div>
                </div>
              )}
            </div>

            {/* 接收状态 */}
            <div className="p-6 bg-slate-50 rounded-lg">
              {receivingFile && receivingProgress ? (
                <div className="space-y-3">
                  <div className="flex items-center justify-between gap-2">
                    <div className="flex items-center gap-2 flex-1 min-w-0">
                      <div className="w-4 h-4 border-2 border-blue-500 border-t-transparent rounded-full animate-spin flex-shrink-0"></div>
                      <span className="font-medium text-blue-600 truncate">{receivingProgress.fileName}</span>
                    </div>
                    <button
                      onClick={async () => {
                        try {
                          await invoke('cancel_file_receiving');
                        } catch (e) {
                          console.error('Failed to cancel receiving:', e);
                        }
                        setReceivingFile(null);
                        setReceivingProgress(null);
                      }}
                      className="px-3 py-1.5 bg-red-500 text-white text-xs rounded hover:bg-red-600 transition-colors flex-shrink-0 whitespace-nowrap"
                    >
                      {t('receive.cancelReceive')}
                    </button>
                  </div>

                  <div className="w-full bg-blue-200 rounded-full h-3 overflow-hidden">
                    <div
                      className="bg-blue-600 h-3 rounded-full transition-all duration-300"
                      style={{ width: `${receivingProgress.progress}%` }}
                    />
                  </div>

                  <div className="flex justify-between text-xs text-slate-600">
                    <span>{receivingProgress.progress.toFixed(1)}%</span>
                    <span>{formatBytes(receivingProgress.received)} / {formatBytes(receivingProgress.total)}</span>
                  </div>
                </div>
              ) : receivingFile ? (
                <div className="text-blue-600 font-medium flex items-center justify-center gap-2">
                  <div className="w-4 h-4 border-2 border-blue-500 border-t-transparent rounded-full animate-spin"></div>
                  {t('receive.receiving')}{receivingFile}
                </div>
              ) : isReceiving ? (
                <div className="text-center">
                  <div className="text-green-600 font-semibold text-lg flex items-center justify-center gap-2">
                    <span className="w-3 h-3 bg-green-500 rounded-full animate-pulse"></span>
                    {t('receive.waiting')}
                  </div>
                  <div className="text-sm text-slate-500 mt-2">
                    {t('receive.instruction')}<span className="font-mono text-slate-700">{localIp}:7878</span>
                  </div>
                </div>
              ) : (
                <div className="text-slate-500 text-center">{t('receive.startup')}</div>
              )}
            </div>

            {/* 传输取消提示 */}
            {receiveCancelledFile && (
              <div className="p-3 bg-amber-50 border border-amber-200 rounded-lg flex items-center justify-between">
                <span className="text-amber-700 text-sm">{t('receive.transferCancelled')}{receiveCancelledFile}</span>
                <button
                  onClick={() => setReceiveCancelledFile(null)}
                  className="text-amber-600 hover:text-amber-800 text-lg font-bold"
                >&times;</button>
              </div>
            )}

            {/* 已接收文件 */}
            {receivedFiles.length > 0 && (
              <div className="pt-4 border-t border-slate-100">
                <label className="block text-sm font-medium text-slate-700 mb-2">{t('receive.receivedFiles')}</label>
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
                  {t('receive.discoveredDevices')} <span className="text-slate-400">({devices.length})</span>
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

        {/* 聊天模式 */}
        {mode === 'chat' && (
          <div className="bg-white rounded-xl border border-slate-200 shadow-sm overflow-hidden">
            {!activeChatIp ? (
              /* 设备选择界面 */
              <div className="p-5 space-y-4">
                <div className="p-4 bg-purple-50 border border-purple-100 rounded-xl">
                  <p className="text-sm font-medium text-slate-600">{t('common.localIp')}</p>
                  <p className="text-xl font-mono font-semibold text-purple-600">{localIp}</p>
                  <p className="text-sm text-purple-600 mt-1 flex items-center gap-1">
                    <span className="w-2 h-2 bg-purple-500 rounded-full animate-pulse"></span>
                    {t('chat.serverStarted')}
                  </p>
                </div>

                {/* 错误提示 */}
                {chatError && (
                  <div className="p-4 bg-red-50 border border-red-200 rounded-lg flex items-center justify-between">
                    <span className="text-red-700">{chatError}</span>
                    <button
                      onClick={() => setChatError(null)}
                      className="text-red-600 hover:text-red-800 text-xl font-bold"
                    >
                      &times;
                    </button>
                  </div>
                )}

                <div>
                  <label className="block text-sm font-medium text-slate-700 mb-2">
                    {t('chat.selectPeer')} <span className="text-slate-400">({devices.length})</span>
                  </label>
                  {devices.length > 0 ? (
                    <div className="space-y-2 max-h-96 overflow-y-auto">
                      {devices.map((device) => (
                        <div
                          key={device.ip}
                          className="flex items-center justify-between p-4 border-2 border-slate-200 rounded-lg hover:border-purple-300 hover:bg-purple-50 transition cursor-pointer"
                          onClick={() => handleStartChat(device)}
                        >
                          <div>
                            <p className="font-medium text-slate-800">{device.hostname}</p>
                            <p className="text-sm text-slate-500 font-mono">{device.ip}</p>
                          </div>
                          <button
                            className="px-4 py-2 text-sm font-medium bg-purple-500 text-white rounded-lg hover:bg-purple-600 transition"
                          >
                            {t('chat.startChat')}
                          </button>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="text-center py-12 text-slate-400">
                      <div className="animate-pulse">{t('chat.searching')}</div>
                      <div className="text-sm mt-2">{t('chat.hint')}</div>
                    </div>
                  )}
                </div>
              </div>
            ) : (
              /* 聊天界面 */
              <div className="flex flex-col h-[600px]">
                {/* 聊天头部 */}
                <div className="p-4 border-b border-slate-200 bg-purple-50 flex items-center justify-between">
                  <div>
                    <p className="font-medium text-slate-800">
                      {devices.find(d => d.ip === activeChatIp)?.hostname || t('chat.unknownDevice')}
                    </p>
                    <div className="flex items-center gap-2 text-sm">
                      <span className="font-mono text-slate-500">{activeChatIp}</span>
                      {chatConnected && (
                        <span className="flex items-center gap-1 text-green-600">
                          <span className="w-2 h-2 bg-green-500 rounded-full"></span>
                          {t('common.connected')}
                        </span>
                      )}
                      {!chatConnected && (
                        <span className="flex items-center gap-1 text-red-600">
                          <span className="w-2 h-2 bg-red-500 rounded-full"></span>
                          {t('common.disconnected')}
                        </span>
                      )}
                    </div>
                  </div>
                  <button
                    onClick={handleDisconnectChat}
                    className="px-4 py-2 text-sm font-medium bg-red-500 text-white rounded-lg hover:bg-red-600 transition"
                  >
                    {t('common.disconnect')}
                  </button>
                </div>

                {/* 错误提示 */}
                {chatError && (
                  <div className="px-4 py-3 bg-red-50 border-b border-red-200 flex items-center justify-between">
                    <span className="text-sm text-red-700">{chatError}</span>
                    <button
                      onClick={() => setChatError(null)}
                      className="text-red-600 hover:text-red-800 text-lg font-bold"
                    >
                      &times;
                    </button>
                  </div>
                )}

                {/* 消息区域 */}
                <div
                  className="flex-1 overflow-y-auto p-4 space-y-3 bg-slate-50"
                  onClick={() => setSelectedMessageIndex(null)}
                >
                  {chatMessages.length === 0 ? (
                    <div className="text-center text-slate-400 py-12">
                      {t('chat.emptyMessage')}
                    </div>
                  ) : (
                    <>
                      {chatMessages.map((msg, idx) => (
                        <div
                          key={idx}
                          className={`flex ${msg.is_me ? 'justify-end' : 'justify-start'}`}
                        >
                          <div className={`max-w-[70%] ${msg.is_me ? 'items-end' : 'items-start'} flex flex-col gap-1`}>
                            <div
                              onClick={(e) => {
                                e.stopPropagation();
                                setSelectedMessageIndex(idx);
                              }}
                              className={`px-4 py-2 rounded-lg cursor-pointer transition ${
                                msg.is_me
                                  ? 'bg-purple-500 text-white rounded-br-none hover:bg-purple-600'
                                  : 'bg-white border border-slate-200 text-slate-800 rounded-bl-none hover:bg-slate-50'
                              }`}
                            >
                              <p className="break-words">{msg.content}</p>
                            </div>
                            <div className={`flex items-center gap-2 px-1 ${msg.is_me ? 'flex-row-reverse' : 'flex-row'}`}>
                              <p className="text-xs text-slate-400">
                                {formatTime(msg.timestamp)}
                              </p>
                              {selectedMessageIndex === idx && (
                                copiedMessageIndex === idx ? (
                                  <span className="text-xs text-green-600 flex items-center gap-0.5">
                                    <span>✓</span>
                                    <span>{t('common.copied')}</span>
                                  </span>
                                ) : (
                                  <button
                                    onClick={(e) => {
                                      e.stopPropagation();
                                      handleCopyMessage(msg.content, idx);
                                    }}
                                    className="text-xs text-slate-500 hover:text-slate-700 transition flex items-center gap-0.5"
                                  >
                                    <span>📋</span>
                                  </button>
                                )
                              )}
                            </div>
                          </div>
                        </div>
                      ))}
                      <div ref={messagesEndRef} />
                    </>
                  )}
                </div>

                {/* 输入区域 */}
                <div className="p-4 border-t border-slate-200 bg-white">
                  <div className="flex gap-2">
                    <input
                      type="text"
                      value={chatInput}
                      onChange={(e) => setChatInput(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey) {
                          e.preventDefault();
                          handleSendChatMessage();
                        }
                      }}
                      placeholder={chatConnected ? t('chat.inputPlaceholder') : t('common.disconnected')}
                      disabled={!chatConnected}
                      className="flex-1 min-w-0 px-4 py-2 border border-slate-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent disabled:bg-slate-100 disabled:text-slate-400"
                    />
                    <button
                      onClick={handleSendChatMessage}
                      disabled={!chatInput.trim() || !chatConnected}
                      className={`px-4 py-2 font-medium rounded-lg transition whitespace-nowrap shrink-0 ${
                        chatInput.trim() && chatConnected
                          ? 'bg-purple-500 text-white hover:bg-purple-600'
                          : 'bg-slate-200 text-slate-400 cursor-not-allowed'
                      }`}
                    >
                      {t('common.send')}
                    </button>
                  </div>
                  <p className="text-xs text-slate-400 mt-2">{t('chat.inputHint')}</p>
                </div>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
