import { useState, useEffect, useRef, useCallback } from 'react';
import {
  Zap, MessageSquare, Cpu, GitBranch, Shield, BarChart3,
  Send, Command, Paperclip, Rocket, Settings,
  Bot, User, CheckCircle2, GitCommit, Lock, BrainCircuit,
  Box, ShieldCheck, Cloud, Activity, AlertTriangle, Key,
  RefreshCw, Wifi, WifiOff, Trash2, Copy, Check
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Checkbox } from '@/components/ui/checkbox';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { toast, Toaster } from 'sonner';

// Empty string = same-origin (works for both Rust-served and vite-proxied)
const API_BASE = (import.meta.env.VITE_API_BASE as string) || '';
const WS_BASE = API_BASE
  ? API_BASE.replace(/^http/, 'ws')
  : `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${window.location.host}`;

// ── Types ──────────────────────────────────────────────────────────────────

interface Message {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: Date;
  metadata?: { local?: boolean; cotTrace?: CoTStep[] };
}

interface CoTStep {
  step: number;
  thought: string;
  action: string;
  observation?: string;
  confidence: number;
}

interface Agent {
  id: string;
  name: string;
  type: 'builder' | 'security' | 'deployer' | 'monitor';
  status: 'idle' | 'working' | 'completed' | 'error';
  task?: string;
}

interface SystemStats {
  memory: number;
  memoryLimit: number;
  cpu: number;
  latency: number;
  connections: number;
  uptime: number;
  totalRequests: number;
  totalTokens: number;
}

// ── Markdown renderer ──────────────────────────────────────────────────────

function renderMarkdown(text: string): string {
  return text
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    .replace(/`([^`]+)`/g, '<code class="bg-slate-800 px-1.5 py-0.5 rounded text-blue-300 text-sm font-mono">$1</code>')
    .replace(/^#{3}\s+(.+)$/gm, '<h3 class="font-semibold text-slate-200 mt-3 mb-1">$1</h3>')
    .replace(/^#{2}\s+(.+)$/gm, '<h2 class="font-bold text-slate-100 mt-4 mb-2 text-lg">$1</h2>')
    .replace(/^#{1}\s+(.+)$/gm, '<h1 class="font-bold text-slate-100 mt-4 mb-2 text-xl">$1</h1>')
    .replace(/^[-•]\s+(.+)$/gm, '<li class="ml-4 text-slate-300">$1</li>')
    .replace(/(<li.*<\/li>\n?)+/g, '<ul class="list-disc list-inside space-y-1 my-2">$&</ul>')
    .replace(/\n\n/g, '</p><p class="mt-2">')
    .replace(/\n/g, '<br/>');
}

// ── Custom hooks ────────────────────────────────────────────────────────────

function useWebSocket(sessionKeyRef: React.MutableRefObject<string>) {
  const [status, setStatus] = useState<'connecting' | 'connected' | 'disconnected'>('disconnected');
  const wsRef = useRef<WebSocket | null>(null);
  const listenersRef = useRef<Set<(data: unknown) => void>>(new Set());
  const reconnectTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const connect = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) return;

    setStatus('connecting');
    const sessionParam = sessionKeyRef.current
      ? `?session=${encodeURIComponent(sessionKeyRef.current)}`
      : '';
    const ws = new WebSocket(`${WS_BASE}/ws${sessionParam}`);
    wsRef.current = ws;

    ws.onopen = () => setStatus('connected');

    ws.onmessage = (e) => {
      try {
        const data = JSON.parse(e.data);
        listenersRef.current.forEach(fn => fn(data));
      } catch { /* ignore malformed */ }
    };

    ws.onclose = () => {
      setStatus('disconnected');
      reconnectTimer.current = setTimeout(connect, 2000);
    };

    ws.onerror = () => ws.close();
  }, [sessionKeyRef]);

  useEffect(() => {
    connect();
    return () => {
      if (reconnectTimer.current) clearTimeout(reconnectTimer.current);
      wsRef.current?.close();
    };
  }, [connect]);

  const send = useCallback((data: unknown) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(data));
      return true;
    }
    return false;
  }, []);

  const subscribe = useCallback((fn: (data: unknown) => void) => {
    listenersRef.current.add(fn);
    return () => { listenersRef.current.delete(fn); };
  }, []);

  return { status, send, subscribe };
}

// ── App ─────────────────────────────────────────────────────────────────────

function App() {
  const sessionKeyRef = useRef<string>(
    localStorage.getItem('aetherclaw_session') ?? ''
  );
  const { status: wsStatus, send: wsSend, subscribe } = useWebSocket(sessionKeyRef);

  const [messages, setMessages] = useState<Message[]>([]);
  const [inputValue, setInputValue] = useState('');
  const [activeTab, setActiveTab] = useState<'chat' | 'agents' | 'deploy' | 'security' | 'metrics'>('chat');
  const [deployPanelOpen, setDeployPanelOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [selectedModel, setSelectedModel] = useState('hybrid');
  const [isThinking, setIsThinking] = useState(false);
  const [showCoT, setShowCoT] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [systemStats, setSystemStats] = useState<SystemStats>({
    memory: 0, memoryLimit: 512, cpu: 0, latency: 0,
    connections: 0, uptime: 0, totalRequests: 0, totalTokens: 0,
  });
  const [agents] = useState<Agent[]>([
    { id: '1', name: 'Builder', type: 'builder', status: 'idle', task: 'Compile Rust binaries' },
    { id: '2', name: 'Security', type: 'security', status: 'idle', task: 'Audit dependencies' },
    { id: '3', name: 'Deployer', type: 'deployer', status: 'idle', task: 'Docker/k8s deployment' },
    { id: '4', name: 'Monitor', type: 'monitor', status: 'idle', task: 'Health checks' },
  ]);
  const [selectedTarget, setSelectedTarget] = useState('x86_64');
  const [optimizeSize, setOptimizeSize] = useState(true);
  const [deployTarget, setDeployTarget] = useState<'docker' | 'kubernetes' | 'bare-metal' | 'edge'>('docker');

  const messagesEndRef = useRef<HTMLDivElement>(null);

  // ── WebSocket message handler ──
  useEffect(() => {
    const unsub = subscribe((raw) => {
      const data = raw as Record<string, unknown>;

      if (data.type === 'init') {
        const key = data.session_key as string;
        sessionKeyRef.current = key;
        localStorage.setItem('aetherclaw_session', key);
        loadHistory(key);
        return;
      }

      if (data.role === 'assistant') {
        setIsThinking(false);
        const msg: Message = {
          id: Date.now().toString(),
          role: 'assistant',
          content: data.content as string,
          timestamp: new Date(),
          metadata: { local: (data.metadata as { local?: boolean })?.local ?? false },
        };
        setMessages(prev => [...prev, msg]);
      }
    });
    return unsub;
  }, [subscribe]);

  // ── Load history ──
  const loadHistory = async (sessionKey: string) => {
    if (!sessionKey) return;
    try {
      const res = await fetch(`${API_BASE}/api/history?session=${encodeURIComponent(sessionKey)}`);
      const json = await res.json();
      if (json.messages?.length) {
        const loaded: Message[] = (json.messages as Array<{ role: string; content: string }>).map((m, i) => ({
          id: `history-${i}`,
          role: m.role as 'user' | 'assistant',
          content: m.content,
          timestamp: new Date(),
        }));
        setMessages(loaded);
      } else {
        setMessages([WELCOME_MESSAGE]);
      }
    } catch {
      setMessages([WELCOME_MESSAGE]);
    }
  };

  // ── Poll status ──
  useEffect(() => {
    const poll = async () => {
      try {
        const start = Date.now();
        const res = await fetch(`${API_BASE}/api/status`);
        const json = await res.json();
        const latency = Date.now() - start;
        setSystemStats({
          memory: json.system?.memory_mb ?? 0,
          memoryLimit: 512,
          cpu: json.system?.cpu_percent ?? 0,
          latency,
          connections: json.connections ?? 0,
          uptime: json.uptime_secs ?? 0,
          totalRequests: json.usage?.total_requests ?? 0,
          totalTokens: json.usage?.total_tokens ?? 0,
        });
      } catch { /* backend might not be running */ }
    };
    poll();
    const id = setInterval(poll, 5000);
    return () => clearInterval(id);
  }, []);

  // ── Auto-scroll ──
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, isThinking]);

  // ── Send message ──
  const handleSend = () => {
    const content = inputValue.trim();
    if (!content || isThinking) return;

    const userMsg: Message = {
      id: Date.now().toString(),
      role: 'user',
      content,
      timestamp: new Date(),
    };
    setMessages(prev => [...prev, userMsg]);
    setInputValue('');
    setIsThinking(true);

    const sent = wsSend({ message: content });
    if (!sent) {
      // HTTP fallback
      fetch(`${API_BASE}/api/chat`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message: content, session_key: sessionKeyRef.current }),
      }).catch(() => {
        setIsThinking(false);
        toast.error('Failed to reach backend. Is AetherClaw running?');
      });
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const clearHistory = async () => {
    setMessages([WELCOME_MESSAGE]);
    toast.success('Chat cleared');
  };

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(id);
      setTimeout(() => setCopied(null), 2000);
    });
  };

  const getAgentIcon = (type: Agent['type']) => {
    switch (type) {
      case 'builder': return <Box className="w-4 h-4" />;
      case 'security': return <ShieldCheck className="w-4 h-4" />;
      case 'deployer': return <Rocket className="w-4 h-4" />;
      case 'monitor': return <Activity className="w-4 h-4" />;
    }
  };

  const getAgentStatusColor = (status: Agent['status']) => {
    switch (status) {
      case 'idle': return 'bg-gray-500';
      case 'working': return 'bg-blue-500 animate-pulse';
      case 'completed': return 'bg-green-500';
      case 'error': return 'bg-red-500';
    }
  };

  const formatUptime = (secs: number) => {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    return h > 0 ? `${h}h ${m}m` : `${m}m`;
  };

  return (
    <div className="h-screen flex flex-col bg-slate-950 text-slate-100 overflow-hidden">
      <Toaster theme="dark" position="top-right" richColors />
      {/* ── Header ── */}
      <header className="sticky top-0 z-50 px-6 py-3 border-b border-slate-800 bg-slate-900/80 backdrop-blur-xl">
        <div className="max-w-7xl mx-auto flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div className="w-9 h-9 bg-gradient-to-br from-blue-600 to-purple-600 rounded-xl flex items-center justify-center shadow-lg shadow-blue-500/20">
              <Zap className="w-5 h-5 text-white" />
            </div>
            <div>
              <h1 className="font-bold text-lg tracking-tight bg-gradient-to-r from-blue-400 to-purple-400 bg-clip-text text-transparent">
                AetherClaw
              </h1>
              <div className="flex items-center gap-3 text-xs text-slate-500 mt-0.5">
                <span className="flex items-center gap-1.5">
                  <span className={`w-1.5 h-1.5 rounded-full ${wsStatus === 'connected' ? 'bg-green-500 animate-pulse' : wsStatus === 'connecting' ? 'bg-yellow-500 animate-pulse' : 'bg-red-500'}`} />
                  {wsStatus === 'connected'
                    ? <span className="text-green-400 font-mono">{systemStats.memory > 0 ? `${systemStats.memory.toFixed(1)}MB` : 'Connected'}</span>
                    : <span className={wsStatus === 'connecting' ? 'text-yellow-400' : 'text-red-400'}>
                        {wsStatus === 'connecting' ? 'Connecting...' : 'Offline'}
                      </span>
                  }
                </span>
                {wsStatus === 'connected' && systemStats.latency > 0 && (
                  <>
                    <span className="w-1 h-1 rounded-full bg-slate-700" />
                    <span className="font-mono text-slate-500">{systemStats.latency}ms</span>
                  </>
                )}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-3">
            <Select value={selectedModel} onValueChange={setSelectedModel}>
              <SelectTrigger className="w-44 bg-slate-800 border-slate-700 text-sm h-8">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="bg-slate-800 border-slate-700">
                <SelectItem value="local">Local (Phi-2 Q4)</SelectItem>
                <SelectItem value="cloud">Cloud (GPT-4o Mini)</SelectItem>
                <SelectItem value="hybrid">Hybrid Auto</SelectItem>
              </SelectContent>
            </Select>

            <Button
              onClick={() => setDeployPanelOpen(true)}
              size="sm"
              className="bg-blue-600 hover:bg-blue-700 shadow-lg shadow-blue-500/20 h-8"
            >
              <Rocket className="w-3.5 h-3.5 mr-1.5" />
              Deploy
            </Button>

            <Button
              variant="ghost"
              size="icon"
              className="text-slate-400 hover:text-slate-200 h-8 w-8"
              onClick={() => setSettingsOpen(true)}
            >
              <Settings className="w-4 h-4" />
            </Button>
          </div>
        </div>
      </header>

      <div className="flex-1 flex max-w-7xl mx-auto w-full overflow-hidden">
        {/* ── Sidebar ── */}
        <aside className="w-56 border-r border-slate-800 bg-slate-900/40 flex flex-col shrink-0">
          <nav className="p-3 space-y-1">
            {(
              [
                { id: 'chat' as const, icon: MessageSquare, label: 'Chat', badge: undefined as number | undefined },
                { id: 'agents' as const, icon: Cpu, label: 'Agents', badge: agents.length as number | undefined },
                { id: 'deploy' as const, icon: GitBranch, label: 'Deployments', badge: undefined as number | undefined },
                { id: 'security' as const, icon: Shield, label: 'Security', badge: undefined as number | undefined },
                { id: 'metrics' as const, icon: BarChart3, label: 'Metrics', badge: undefined as number | undefined },
              ]
            ).map(({ id, icon: Icon, label, badge }) => (
              <button
                key={id}
                onClick={() => setActiveTab(id)}
                className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors ${
                  activeTab === id
                    ? 'bg-blue-600/10 text-blue-400 border border-blue-600/20'
                    : 'text-slate-400 hover:bg-slate-800 hover:text-slate-200'
                }`}
              >
                <Icon className="w-4 h-4" />
                <span>{label}</span>
                {badge !== undefined && (
                  <span className="ml-auto text-xs bg-slate-800 px-1.5 py-0.5 rounded-full text-slate-500">{badge}</span>
                )}
              </button>
            ))}
          </nav>

          <div className="mt-auto p-3 border-t border-slate-800">
            <div className="bg-slate-800/50 rounded-lg p-3 border border-slate-700/50 space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-medium text-slate-400">System</span>
                <div className="flex items-center gap-1">
                  {wsStatus === 'connected' ? (
                    <Wifi className="w-3 h-3 text-green-400" />
                  ) : (
                    <WifiOff className="w-3 h-3 text-red-400" />
                  )}
                  <span className={`w-2 h-2 rounded-full ${wsStatus === 'connected' ? 'bg-green-500' : 'bg-red-500'}`} />
                </div>
              </div>
              <div className="space-y-1.5 text-xs font-mono">
                <div>
                  <div className="flex justify-between text-slate-500 mb-1">
                    <span>RAM</span>
                    <span className="text-slate-300">
                      {systemStats.memory > 0 ? `${systemStats.memory.toFixed(1)}MB` : '—'}
                    </span>
                  </div>
                  <div className="w-full bg-slate-700 rounded-full h-1">
                    <div
                      className="bg-blue-500 h-1 rounded-full transition-all duration-500"
                      style={{ width: `${Math.min((systemStats.memory / systemStats.memoryLimit) * 100, 100)}%` }}
                    />
                  </div>
                </div>
                <div className="flex justify-between text-slate-500">
                  <span>Uptime</span>
                  <span className="text-slate-300">
                    {systemStats.uptime > 0 ? formatUptime(systemStats.uptime) : '—'}
                  </span>
                </div>
              </div>
            </div>
          </div>
        </aside>

        {/* ── Main Content ── */}
        <main className="flex-1 flex flex-col bg-slate-950 overflow-hidden">
          {/* Chat tab */}
          {activeTab === 'chat' && (
            <>
              <div className="flex-1 overflow-hidden flex flex-col">
                <ScrollArea className="flex-1 p-5">
                  <div className="space-y-5 max-w-3xl">
                    {messages.map((message) => (
                      <div key={message.id} className="flex gap-3 animate-in slide-in-from-bottom-2 group">
                        <div className={`w-7 h-7 rounded-full flex items-center justify-center shrink-0 mt-0.5 ${
                          message.role === 'user'
                            ? 'bg-blue-600'
                            : message.role === 'system'
                            ? 'bg-gradient-to-br from-purple-500 to-pink-600'
                            : 'bg-gradient-to-br from-green-500 to-emerald-600'
                        }`}>
                          {message.role === 'user'
                            ? <User className="w-3.5 h-3.5 text-white" />
                            : <Bot className="w-3.5 h-3.5 text-white" />
                          }
                        </div>

                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 mb-1">
                            <span className="text-sm font-medium text-slate-200">
                              {message.role === 'user' ? 'You' : 'AetherClaw'}
                            </span>
                            {message.metadata?.local && (
                              <Badge variant="outline" className="text-[10px] border-green-800 text-green-400 bg-green-950/30 h-4">
                                Local
                              </Badge>
                            )}
                            {message.metadata?.cotTrace && (
                              <Button
                                variant="ghost"
                                size="sm"
                                className="h-4 text-[10px] text-blue-400 px-1"
                                onClick={() => setShowCoT(!showCoT)}
                              >
                                <BrainCircuit className="w-3 h-3 mr-1" />
                                CoT
                              </Button>
                            )}
                            <button
                              onClick={() => copyToClipboard(message.content, message.id)}
                              className="ml-auto opacity-0 group-hover:opacity-100 transition-opacity p-1 hover:bg-slate-800 rounded"
                            >
                              {copied === message.id
                                ? <Check className="w-3 h-3 text-green-400" />
                                : <Copy className="w-3 h-3 text-slate-500" />
                              }
                            </button>
                          </div>

                          <div
                            className="text-sm text-slate-300 leading-relaxed"
                            dangerouslySetInnerHTML={{ __html: renderMarkdown(message.content) }}
                          />

                          {showCoT && message.metadata?.cotTrace && (
                            <Card className="mt-3 bg-slate-800/50 border-slate-700/50">
                              <CardHeader className="py-2 px-3">
                                <CardTitle className="text-xs flex items-center gap-2 text-slate-400">
                                  <BrainCircuit className="w-3 h-3" />
                                  Chain-of-Thought Trace
                                </CardTitle>
                              </CardHeader>
                              <CardContent className="py-2 px-3 space-y-2">
                                {message.metadata.cotTrace.map((step, idx) => (
                                  <div key={idx} className="text-xs border-l-2 border-slate-700 pl-3 py-1">
                                    <div className="flex items-center gap-2 text-blue-400">
                                      <GitCommit className="w-3 h-3" />
                                      <span>Step {step.step} [{Math.round(step.confidence * 100)}%]</span>
                                    </div>
                                    <div className="text-slate-400 mt-0.5">{step.thought}</div>
                                    {step.observation && (
                                      <div className="text-green-400 mt-0.5">→ {step.observation}</div>
                                    )}
                                  </div>
                                ))}
                              </CardContent>
                            </Card>
                          )}
                        </div>
                      </div>
                    ))}

                    {/* Thinking indicator */}
                    {isThinking && (
                      <div className="flex gap-3 animate-in fade-in">
                        <div className="w-7 h-7 rounded-full bg-gradient-to-br from-green-500 to-emerald-600 flex items-center justify-center shrink-0">
                          <Bot className="w-3.5 h-3.5 text-white" />
                        </div>
                        <div className="flex items-center gap-2 text-slate-400 text-sm">
                          <div className="flex gap-1">
                            <span className="w-1.5 h-1.5 bg-slate-500 rounded-full animate-bounce" style={{ animationDelay: '0ms' }} />
                            <span className="w-1.5 h-1.5 bg-slate-500 rounded-full animate-bounce" style={{ animationDelay: '150ms' }} />
                            <span className="w-1.5 h-1.5 bg-slate-500 rounded-full animate-bounce" style={{ animationDelay: '300ms' }} />
                          </div>
                          <span className="text-xs text-slate-500">Thinking...</span>
                        </div>
                      </div>
                    )}

                    <div ref={messagesEndRef} />
                  </div>
                </ScrollArea>
              </div>

              {/* Input bar */}
              <div className="border-t border-slate-800 bg-slate-900/40 p-4 backdrop-blur-sm">
                <div className="max-w-3xl mx-auto">
                  <div className="relative flex items-center gap-2">
                    <Command className="absolute left-4 w-4 h-4 text-slate-500 pointer-events-none" />
                    <Input
                      value={inputValue}
                      onChange={e => setInputValue(e.target.value)}
                      onKeyDown={handleKeyDown}
                      placeholder="Command AetherClaw to build, deploy, analyze, or ask anything…"
                      className="flex-1 bg-slate-800 border-slate-700 text-slate-100 pl-11 pr-4 py-5 rounded-xl focus:ring-2 focus:ring-blue-500/40 placeholder:text-slate-600"
                      disabled={isThinking}
                    />
                    <div className="flex items-center gap-1.5 absolute right-2">
                      <Button variant="ghost" size="icon" className="text-slate-500 hover:text-slate-300 h-8 w-8">
                        <Paperclip className="w-3.5 h-3.5" />
                      </Button>
                      <Button
                        onClick={handleSend}
                        disabled={isThinking || !inputValue.trim()}
                        size="icon"
                        className="bg-blue-600 hover:bg-blue-700 disabled:opacity-40 h-8 w-8"
                      >
                        <Send className="w-3.5 h-3.5" />
                      </Button>
                    </div>
                  </div>
                  <div className="flex items-center justify-between mt-2 text-xs text-slate-600 px-1">
                    <span>
                      <kbd className="bg-slate-800 px-1.5 py-0.5 rounded border border-slate-700 font-mono">Enter</kbd> to send
                    </span>
                    <div className="flex items-center gap-3">
                      <button
                        onClick={clearHistory}
                        className="flex items-center gap-1 text-slate-600 hover:text-slate-400 transition-colors"
                      >
                        <Trash2 className="w-3 h-3" />
                        Clear
                      </button>
                      <span className="flex items-center gap-1">
                        <Lock className="w-3 h-3" />
                        Local-first
                      </span>
                    </div>
                  </div>
                </div>
              </div>
            </>
          )}

          {/* Agents tab */}
          {activeTab === 'agents' && (
            <div className="flex-1 p-6 overflow-auto">
              <div className="max-w-4xl">
                <h2 className="text-xl font-bold mb-5">Multi-Agent System</h2>
                <div className="grid grid-cols-2 gap-4">
                  {agents.map(agent => (
                    <Card key={agent.id} className="bg-slate-800/50 border-slate-700">
                      <CardHeader className="pb-2">
                        <div className="flex items-center justify-between">
                          <div className="flex items-center gap-3">
                            <div className={`w-9 h-9 rounded-lg flex items-center justify-center ${
                              agent.type === 'builder' ? 'bg-blue-500/20 text-blue-400' :
                              agent.type === 'security' ? 'bg-green-500/20 text-green-400' :
                              agent.type === 'deployer' ? 'bg-purple-500/20 text-purple-400' :
                              'bg-orange-500/20 text-orange-400'
                            }`}>
                              {getAgentIcon(agent.type)}
                            </div>
                            <div>
                              <CardTitle className="text-sm">{agent.name} Agent</CardTitle>
                              <p className="text-xs text-slate-400">{agent.task}</p>
                            </div>
                          </div>
                          <div className={`w-2.5 h-2.5 rounded-full ${getAgentStatusColor(agent.status)}`} />
                        </div>
                      </CardHeader>
                      <CardContent className="pt-0">
                        <span className="text-xs text-slate-500 capitalize">Status: {agent.status}</span>
                      </CardContent>
                    </Card>
                  ))}
                </div>

                <Card className="mt-5 bg-slate-800/50 border-slate-700">
                  <CardHeader className="pb-3">
                    <CardTitle className="text-sm flex items-center gap-2">
                      <BrainCircuit className="w-4 h-4" />
                      Pipeline: Build → Security → Deploy → Monitor
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="flex items-center gap-3">
                      {(['Builder', 'Security', 'Deployer', 'Monitor'] as const).map((step, idx) => (
                        <div key={step} className="flex items-center gap-3">
                          <div className="flex flex-col items-center">
                            <div className="w-10 h-10 rounded-full flex items-center justify-center bg-slate-700 text-slate-400">
                              <CheckCircle2 className="w-5 h-5" />
                            </div>
                            <span className="text-xs mt-1.5 text-slate-500">{step}</span>
                          </div>
                          {idx < 3 && <div className="w-6 h-0.5 bg-slate-700" />}
                        </div>
                      ))}
                    </div>
                    <p className="text-xs text-slate-500 mt-4">
                      Say <code className="bg-slate-800 px-1.5 py-0.5 rounded text-blue-400">"build and deploy"</code> in chat to trigger the full pipeline.
                    </p>
                  </CardContent>
                </Card>
              </div>
            </div>
          )}

          {/* Deployments tab */}
          {activeTab === 'deploy' && (
            <div className="flex-1 p-6 overflow-auto">
              <div className="max-w-4xl">
                <h2 className="text-xl font-bold mb-5">Deployments</h2>
                <div className="space-y-4">
                  <Card className="bg-slate-800/50 border-slate-700">
                    <CardContent className="pt-4">
                      <p className="text-sm text-slate-400">
                        No deployments yet. Use the chat to run <code className="bg-slate-900 px-1.5 py-0.5 rounded text-blue-400 text-xs">build and deploy</code>, or click the Deploy button above.
                      </p>
                    </CardContent>
                  </Card>
                </div>
              </div>
            </div>
          )}

          {/* Security tab */}
          {activeTab === 'security' && (
            <div className="flex-1 p-6 overflow-auto">
              <div className="max-w-4xl">
                <h2 className="text-xl font-bold mb-5">Security Audit</h2>
                <div className="grid grid-cols-2 gap-4">
                  <Card className="bg-slate-800/50 border-slate-700">
                    <CardHeader className="pb-2">
                      <CardTitle className="text-sm flex items-center gap-2">
                        <ShieldCheck className="w-4 h-4 text-green-400" />
                        Workspace Sandbox
                      </CardTitle>
                    </CardHeader>
                    <CardContent>
                      <p className="text-green-400 text-sm">cap-std filesystem isolation active</p>
                    </CardContent>
                  </Card>
                  <Card className="bg-slate-800/50 border-slate-700">
                    <CardHeader className="pb-2">
                      <CardTitle className="text-sm flex items-center gap-2">
                        <AlertTriangle className="w-4 h-4 text-yellow-400" />
                        Dependency Audit
                      </CardTitle>
                    </CardHeader>
                    <CardContent>
                      <p className="text-slate-400 text-sm">Run <code className="bg-slate-900 px-1.5 text-blue-400 rounded text-xs">cargo audit</code> locally to check</p>
                    </CardContent>
                  </Card>
                  <Card className="bg-slate-800/50 border-slate-700">
                    <CardHeader className="pb-2">
                      <CardTitle className="text-sm flex items-center gap-2">
                        <Lock className="w-4 h-4 text-blue-400" />
                        API Key Storage
                      </CardTitle>
                    </CardHeader>
                    <CardContent>
                      <p className="text-slate-400 text-sm">Keys stored in <code className="bg-slate-900 px-1.5 text-blue-400 rounded text-xs">~/.aetherclaw/config.toml</code></p>
                    </CardContent>
                  </Card>
                  <Card className="bg-slate-800/50 border-slate-700">
                    <CardHeader className="pb-2">
                      <CardTitle className="text-sm flex items-center gap-2">
                        <Shield className="w-4 h-4 text-purple-400" />
                        Command Allowlist
                      </CardTitle>
                    </CardHeader>
                    <CardContent>
                      <p className="text-slate-400 text-sm">Dangerous exec patterns blocked by policy</p>
                    </CardContent>
                  </Card>
                </div>
              </div>
            </div>
          )}

          {/* Metrics tab */}
          {activeTab === 'metrics' && (
            <div className="flex-1 p-6 overflow-auto">
              <div className="max-w-4xl">
                <div className="flex items-center justify-between mb-5">
                  <h2 className="text-xl font-bold">System Metrics</h2>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => fetch(`${API_BASE}/api/status`)}
                    className="text-slate-400 gap-1.5"
                  >
                    <RefreshCw className="w-3.5 h-3.5" />
                    Refresh
                  </Button>
                </div>
                <div className="grid grid-cols-2 gap-4">
                  {[
                    { label: 'Memory', value: systemStats.memory > 0 ? `${systemStats.memory.toFixed(1)} MB` : '—', sub: `of ${systemStats.memoryLimit} MB limit` },
                    { label: 'CPU', value: systemStats.cpu > 0 ? `${systemStats.cpu.toFixed(1)}%` : '—', sub: 'Process usage' },
                    { label: 'Latency', value: systemStats.latency > 0 ? `${systemStats.latency} ms` : '—', sub: 'API round-trip' },
                    { label: 'Uptime', value: systemStats.uptime > 0 ? formatUptime(systemStats.uptime) : '—', sub: 'Since last start' },
                    { label: 'Connections', value: String(systemStats.connections), sub: 'Active WebSocket' },
                    { label: 'Requests', value: String(systemStats.totalRequests), sub: 'Total handled' },
                    { label: 'Tokens', value: systemStats.totalTokens > 0 ? systemStats.totalTokens.toLocaleString() : '—', sub: 'Cloud tokens used' },
                  ].map(m => (
                    <Card key={m.label} className="bg-slate-800/50 border-slate-700">
                      <CardHeader className="pb-2">
                        <CardTitle className="text-xs text-slate-400">{m.label}</CardTitle>
                      </CardHeader>
                      <CardContent className="pt-0">
                        <div className="text-2xl font-mono text-slate-100">{m.value}</div>
                        <p className="text-xs text-slate-500 mt-1">{m.sub}</p>
                      </CardContent>
                    </Card>
                  ))}
                </div>
              </div>
            </div>
          )}
        </main>
      </div>

      {/* ── Deploy Dialog ── */}
      <Dialog open={deployPanelOpen} onOpenChange={setDeployPanelOpen}>
        <DialogContent className="bg-slate-900 border-slate-800 text-slate-100 max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2 text-base">
              <Rocket className="w-4 h-4" />
              Deploy Pipeline
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-5">
            <div className="space-y-2">
              <label className="text-sm font-medium flex items-center gap-2">
                <Box className="w-4 h-4 text-blue-400" />
                Build Target
              </label>
              <Select value={selectedTarget} onValueChange={setSelectedTarget}>
                <SelectTrigger className="bg-slate-800 border-slate-700">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="bg-slate-800 border-slate-700">
                  <SelectItem value="x86_64">x86_64-unknown-linux-musl</SelectItem>
                  <SelectItem value="aarch64">aarch64-unknown-linux-musl</SelectItem>
                  <SelectItem value="riscv64">riscv64gc-unknown-linux-gnu</SelectItem>
                </SelectContent>
              </Select>
              <div className="flex items-center gap-2 text-sm text-slate-400">
                <Checkbox
                  id="optimize"
                  checked={optimizeSize}
                  onCheckedChange={v => setOptimizeSize(v === true)}
                />
                <label htmlFor="optimize">Optimize for size (opt-level=z)</label>
              </div>
            </div>

            <div className="space-y-2">
              <label className="text-sm font-medium flex items-center gap-2">
                <Cloud className="w-4 h-4 text-purple-400" />
                Deployment Target
              </label>
              <div className="grid grid-cols-2 gap-2">
                {(['docker', 'kubernetes', 'bare-metal', 'edge'] as const).map(t => (
                  <button
                    key={t}
                    onClick={() => setDeployTarget(t)}
                    className={`p-2.5 rounded-lg border text-sm font-medium capitalize transition-colors ${
                      deployTarget === t
                        ? 'border-blue-500 bg-blue-500/10 text-blue-400'
                        : 'border-slate-700 text-slate-400 hover:border-slate-600'
                    }`}
                  >
                    {t === 'bare-metal' ? 'Bare Metal' : t.charAt(0).toUpperCase() + t.slice(1)}
                  </button>
                ))}
              </div>
            </div>

            <Button
              className="w-full bg-blue-600 hover:bg-blue-700"
              onClick={() => {
                const cmd = `build and deploy for ${selectedTarget} using ${deployTarget}${optimizeSize ? ' with size optimization' : ''}`;
                setDeployPanelOpen(false);
                setActiveTab('chat');
                setInputValue(cmd);
                toast.info('Deployment command staged — press Enter to run');
              }}
            >
              <Rocket className="w-4 h-4 mr-2" />
              Stage Deployment
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {/* ── Settings Dialog ── */}
      <Dialog open={settingsOpen} onOpenChange={setSettingsOpen}>
        <DialogContent className="bg-slate-900 border-slate-800 text-slate-100 max-w-lg">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2 text-base">
              <Settings className="w-4 h-4" />
              Settings
            </DialogTitle>
          </DialogHeader>
          <Tabs defaultValue="api">
            <TabsList className="bg-slate-800 border-slate-700 w-full">
              <TabsTrigger value="api" className="flex-1 text-xs">API Keys</TabsTrigger>
              <TabsTrigger value="config" className="flex-1 text-xs">Config File</TabsTrigger>
              <TabsTrigger value="about" className="flex-1 text-xs">About</TabsTrigger>
            </TabsList>

            <TabsContent value="api" className="space-y-4 pt-3">
              <p className="text-xs text-slate-400">
                API keys are stored in <code className="bg-slate-800 px-1.5 py-0.5 rounded text-blue-300">~/.aetherclaw/config.toml</code>
              </p>
              {[
                { label: 'OpenAI / Compatible', placeholder: 'sk-...' },
                { label: 'Anthropic', placeholder: 'sk-ant-...' },
                { label: 'OpenRouter', placeholder: 'sk-or-...' },
                { label: 'Brave Search', placeholder: 'BSA...' },
              ].map(({ label, placeholder }) => (
                <div key={label} className="space-y-1.5">
                  <label className="text-xs font-medium text-slate-400 flex items-center gap-1.5">
                    <Key className="w-3 h-3" />
                    {label}
                  </label>
                  <Input
                    type="password"
                    placeholder={placeholder}
                    className="bg-slate-800 border-slate-700 text-slate-300 text-sm"
                  />
                </div>
              ))}
              <p className="text-xs text-slate-500">
                Saving via UI coming soon. For now, edit config.toml and restart.
              </p>
            </TabsContent>

            <TabsContent value="config" className="pt-3">
              <div className="bg-slate-800/60 rounded-lg p-4 font-mono text-xs text-slate-300 space-y-1.5 border border-slate-700">
                <p className="text-slate-500"># ~/.aetherclaw/config.toml</p>
                <p><span className="text-purple-400">[gateway]</span></p>
                <p className="pl-2"><span className="text-blue-400">host</span> = <span className="text-green-400">"0.0.0.0"</span></p>
                <p className="pl-2"><span className="text-blue-400">port</span> = <span className="text-orange-400">8080</span></p>
                <p><span className="text-purple-400">[llm]</span></p>
                <p className="pl-2 text-slate-500"># Add model_list entries here</p>
                <p><span className="text-purple-400">[[llm.model_list]]</span></p>
                <p className="pl-2"><span className="text-blue-400">model_name</span> = <span className="text-green-400">"gpt-4o-mini"</span></p>
                <p className="pl-2"><span className="text-blue-400">model</span> = <span className="text-green-400">"openai/gpt-4o-mini"</span></p>
                <p className="pl-2"><span className="text-blue-400">api_key</span> = <span className="text-green-400">"sk-..."</span></p>
              </div>
            </TabsContent>

            <TabsContent value="about" className="pt-3 space-y-3">
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 bg-gradient-to-br from-blue-600 to-purple-600 rounded-xl flex items-center justify-center">
                  <Zap className="w-5 h-5 text-white" />
                </div>
                <div>
                  <p className="font-bold text-slate-200">AetherClaw v0.1.0</p>
                  <p className="text-xs text-slate-400">Rust edge AI agent — PicoClaw successor</p>
                </div>
              </div>
              <div className="text-xs text-slate-500 space-y-1">
                <p>• Multi-agent CoT/ReAct loop</p>
                <p>• Local-first inference (llama-cpp-rs)</p>
                <p>• cap-std workspace sandboxing</p>
                <p>• Axum WebSocket streaming</p>
                <p>• SQLite conversation memory</p>
                <p>• Telegram, Discord, Web channels</p>
              </div>
            </TabsContent>
          </Tabs>
        </DialogContent>
      </Dialog>
    </div>
  );
}

const WELCOME_MESSAGE: Message = {
  id: 'welcome',
  role: 'system',
  content: 'Welcome to **AetherClaw** — edge AI command center.\n\nRunning locally with ultra-low memory footprint. Try:\n- `"Build and deploy the latest release"`\n- `"Run a security audit"`\n- `"What files are in the workspace?"`\n- Any question you\'d ask a dev assistant',
  timestamp: new Date(),
};

export default App;
