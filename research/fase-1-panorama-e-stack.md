# Pesquisa Fase 1 — Legendas ao vivo do Discord em overlay no jogo

**Data:** 15/08/2026 · **Método:** 4 frentes de pesquisa paralelas (mundo dos jogos, fora dos jogos, stack técnica, projetos open-source) · **Status:** Fase 1 concluída

---

## TL;DR

1. **A demanda é real, documentada e ignorada há 6+ anos.** Xbox (2017/2021), PS5 (2020) e Switch 2 (2025) já entregam transcrição de voice chat no nível da plataforma — com overlay e atribuição por jogador no caso do Xbox. O Discord, a maior plataforma de voz do PC gaming, não tem nada: 8+ threads de pedidos abertas desde 2019/2020 (inclusive uma em português), petição no Change.org, e a resposta oficial é que só constroem o que tem "alta demanda".
2. **Sua hipótese sobre o timing estava certa.** Todas as tentativas pré-2022 morreram porque o STT gratuito era ruim (DeepSpeech, Vosk) e o bom era cloud pago por minuto (Google, wit.ai). O Whisper (set/2022) + faster-whisper/whisper.cpp (2023) destravaram transcrição local, gratuita e quase em tempo real. A onda atual de projetos é toda de 2023–2026.
3. **Surgiu um concorrente direto em março de 2026: CaptionsRush.** Feito por um dev com deficiência auditiva, faz exatamente isso (legendas do Discord em overlay no jogo, com falantes identificados). Mas: é fechado, a melhor qualidade fica atrás de cloud pago (~$0,02/min), o modo local é fraco fora do inglês (ruim para pt-BR), e o modo com falantes exige que o usuário crie o próprio bot no Developer Portal. A posição "grátis, open-source, zero fricção, pt-BR de primeira classe" está vaga.
4. **A descoberta técnica central: não precisa de bot nem de IA de diarização.** O client do Discord expõe localmente, via RPC, os eventos `SPEAKING_START/STOP` com o `user_id` de quem está falando (é assim que o Overlayed e o StreamKit funcionam). Combinando isso com captura de áudio só do processo do Discord (Process Loopback API do Windows), você tem legendas com nome/avatar/cor exatos do Discord, sem bot, sem consentimento cerimonial, imune à criptografia E2EE que quebrou os bots de voz em março de 2026.
5. **Stack recomendada:** Tauri v2 (Rust) + captura WASAPI process-loopback + atribuição via Discord RPC + STT local com streaming real (NVIDIA Nemotron-3.5-ASR-streaming-0.6b — inglês E português, licença permissiva, roda em CPU/iGPU sem brigar com o jogo) + overlay como janela transparente click-through (nunca injeção → zero risco de anti-cheat). Cloud opcional: AssemblyAI (~US$ 13,50/mês para 3h/dia).

---

## 1. Veredito sobre a ideia

| Pergunta | Resposta |
|---|---|
| A dor existe? | **Sim, comprovada** — relatos de exclusão (kickados de lobby, evitar jogos com voz), threads de pedidos desde 2019, pesquisa acadêmica brasileira validando exatamente as necessidades que o app resolve |
| O timing é bom? | **Sim** — o gargalo técnico quebrou entre 2022–2024; modelos streaming com pt-BR de qualidade apareceram em 2025–2026 |
| Já existe? | **Quase** — 1 concorrente direto novo e fechado (CaptionsRush, mar/2026); todo o resto falha em pelo menos 1 dos 4 requisitos |
| Dá pra diferenciar? | **Sim** — grátis + open-source + pt-BR + zero-fricção (sem criar bot) + UX nativa do Discord |
| Maior risco? | Discord lançar nativo (improvável no curto prazo: reformularam o overlay em mar/2025 sem captions) e fragilidade das APIs não documentadas |

Os **4 requisitos** que nenhuma solução atual combina (exceto o CaptionsRush, com ressalvas): tempo real + identificação de quem fala + overlay dentro do jogo + áudio do Discord.

---

## 2. O que já existe — mundo dos jogos

### 2.1 Consoles (o precedente que valida tudo)

| Plataforma | O que tem | Desde | Atribuição de falante |
|---|---|---|---|
| **Xbox** | Game chat transcription (STT+TTS em jogos suportados); **Party Chat transcription em overlay ajustável sobre o gameplay** | 2017 / 2021 | **Sim — gamertag por linha** |
| **PS5** | Chat transcription (voz do party → texto no card, fixável); type-to-speak | Lançamento (2020) | Por jogador no card. 6 idiomas — **sem português** |
| **Switch 2** | GameChat com transcript; imprensa elogiou "capacidade de distinguir falantes específicos" | Jun/2025 | Sim |

Jogos individuais (Halo Infinite, Apex, New World) têm STT interno — mas a conversa social de verdade acontece no Discord, não no voice do jogo.

### 2.2 CaptionsRush — o concorrente direto ⚠️

- **O quê:** legendas em tempo real em overlay customizável (posição/tamanho/cor/opacidade), lançado ~março/2026 por **Oren Lande**, dev com deficiência auditiva que queria jogar com os filhos ouvintes. Cobertura do Can I Play That em 26/03/2026.
- **Dois modos:** (a) **bot** — o usuário cria um bot pessoal no Developer Portal (~5 min de setup técnico); o bot recebe o stream separado de cada participante → legendas limpas por falante; (b) **áudio do sistema** — captura tudo misturado, funciona com qualquer coisa, sem falantes confiáveis.
- **Preço:** tier local/offline grátis (Whisper + NVIDIA NeMo, "punhado de idiomas", precisão visivelmente pior — o review aponta fraqueza "particularmente com falantes não nativos de inglês"); cloud pago ~US$ 0,02/min após 60–120 min grátis. Overlay só no Windows. **Não é open-source.**
- **Onde ele é vencível:** custo (a comunidade considera que acessibilidade "tem que ser 100% grátis" — petição), pt-BR (fraco no modo local dele; nosso default local já teria pt-BR de qualidade), fricção (criar bot vs. detectar o client automaticamente via RPC), abertura (ferramentas grátis fechadas morrem: Web Captioner †2023, HarmonyInSilence †2025 — open-source mitiga o "fator ônibus").

### 2.3 Bots de transcrição do Discord (todos falham no mesmo ponto)

| Bot | Status | Falantes | Limite estrutural |
|---|---|---|---|
| **Scriptly** | Ativo, 95.000+ servidores | Premium | Texto vai para um **canal de texto** → alt-tab |
| **Scripty** | Ativo, open-source, STT offline, ~55 idiomas | Sem rótulo por linha | Canal de texto |
| **SeaVoice** | Ativo, grátis | Sim | Canal de texto |
| **Craig** | Ativo desde 2017 (ISC) | Sim (1 faixa por falante) | **Gravador**, não legendas ao vivo |
| Textional Voice, MeetMind, Spext, Ablebot | **Mortos** (teste 2026) | — | — |
| HarmonyInSilence (Deepgram, p/ surdos) | **Arquivado abr/2025** | Não | Hackathon, nunca virou produto |

**A última milha — renderizar as legendas dentro do jogo — nunca foi construída em open-source.** Esse é exatamente o vazio.

### 2.4 Outros vazios

- **Overwolf:** zero apps de legendas na loja (só voice changer e tradução de texto). Nicho vazio; possível canal de distribuição na fase 2 (overlay whitelisted em anti-cheats, mas exige aprovação e trava monetização).
- **Steam:** atualização de acessibilidade 2025 foi só UI (escala, contraste); nada de captions no voice chat. ~2,3% dos jogos marcados como legendados.
- **Mods de client (BetterDiscord/Vencord):** nenhum plugin de legendas ao vivo existe; e mods violam o ToS do Discord — não é base aceitável para ferramenta de acessibilidade. Um app externo usando superfícies oficiais é inclusive um diferencial de marketing ("100% dentro das regras").

---

## 3. O que já existe — fora dos jogos

### Matriz de capacidades

| Solução | Tempo real | Nomes de falante | Overlay sobre jogo | Pega áudio do Discord | Preço | pt-BR |
|---|---|---|---|---|---|---|
| **Windows 11 Live Captions** | Sim (delay perceptível) | **Não** | Janela flutuante; morre em fullscreen exclusivo | Sim (todo o áudio misturado, jogo incluso) | Grátis | **Sim** |
| macOS Live Captions | Sim | Só no FaceTime | Não | Misturado | Grátis (Apple Silicon) | Limitado |
| Android Live Caption / Live Transcribe | Sim | Não | — | Não confiável | Grátis | Parcial |
| **Ava** (app p/ surdos) | Sim | **Sim — cada pessoa entra pelo próprio device** | Caixa flutuante, não game-aware | Não | Grátis; premium US$ 9,99–14,99/mês | Multi-idioma |
| Otter.ai | Sim (reuniões) | Sim (imperfeito) | Não | Não | 300 min grátis/mês | Fraco |
| Web Captioner | — | — | — | — | **Morreu out/2023** | — |
| InnoCaption / Rogervoice | Sim | — | Não | Não (só telefonia) | Grátis (FCC/EUA) | Rogervoice sim |
| Zoom / Teams / Meet | Sim | Sim (**Teams é a referência**: nome em cada linha) | Não | Não | Incluído | Sim |

**Leituras importantes dessa matriz:**

- O **Windows Live Captions é o "concorrente grátis" a ser batido** — e é o workaround mais usado hoje. Suas falhas estruturais são as nossas features: ele mistura o áudio do jogo na transcrição (música/efeitos poluem tudo), não diz quem falou, e não funciona em fullscreen exclusivo.
- **O segredo de quem faz atribuição de falante bem: separação de canal, não IA.** Ava (um device por pessoa), Teams/Zoom (um stream por participante), Xbox (nível de plataforma), bots do Discord (um stream por usuário). Ninguém diarializa áudio misturado com sucesso em tempo real. **O Discord nos dá a separação de graça** — essa é a vantagem estrutural de construir sobre ele.
- **Expectativa de precisão da comunidade:** ~90% é tolerado em contexto casual ("prefiro 60% de precisão e participar" vs. "legenda ruim é igual a legenda nenhuma" — as duas atitudes existem). Para callouts de jogo, **nome do falante e latência importam mais que os últimos 5% de precisão**.

---

## 4. Como surdos/ensurdecidos se viram hoje (a dor documentada)

1. **Evitam jogos dependentes de voz** ("evito MMOs porque dependem de voice chat").
2. **São excluídos** — relatos abundantes de serem kickados de lobbies ou de times debandarem ao descobrir que o jogador não usa voz.
3. **Imploram por ferramentas que não existem** — thread no Steam pedindo exatamente este app terminou sem nenhuma sugestão funcional.
4. **Pedem para os amigos digitarem** — lento, constrangedor, desvantagem competitiva.
5. **Rodam o Windows Live Captions** em janela borderless — com poluição de áudio do jogo, delay e sem nomes.
6. **Apontam o celular com Ava/Live Transcribe para a caixa de som** (gambiarra física real, documentada no blog da própria Ava).

**Demanda além do nicho (efeito curb-cut, como você previu):** legendas em voice chat também são pedidas por pessoas com transtorno de processamento auditivo, TDAH, autismo e por falantes não nativos — o mesmo padrão do indicador visual de som do Fortnite.

### Brasil 🇧🇷

- Thread em português no feedback do Discord: **"Legendas ao vivo durante chamadas"** — aberta, sem resposta.
- **Pesquisa acadêmica da UFSM (Naidon, Bernardi & Cordenonsi, RENOTE v.21, 2023):** diretrizes de acessibilidade para surdos em jogos, construídas com uma comunidade de jogadores surdos *dentro do Discord*. Diretrizes validadas que são literalmente o nosso app: **1.5 "indicação visual de qual personagem está falando"**, 1.6 legendas customizáveis, 5.4 "transcrição de fala na comunicação multiplayer". Reclamação recorrente dos respondentes: conteúdo "sem identificar quem está falando".
- Mídia tech brasileira cobre TTS/voice changer do Discord, mas legenda de voice chat é assunto praticamente inexistente em pt-BR — espaço aberto.

---

## 5. Por que agora (linha do tempo do gargalo)

| Época | O que tinha | Por que morreu |
|---|---|---|
| 2019–2021 | Bots com wit.ai / Google STT (DiscordSpeechBot, discord-stt); Scripty v1 sobre Mozilla DeepSpeech | Cloud pago por minuto insustentável; DeepSpeech abandonado pela Mozilla em 2021 com precisão ruim |
| Jan/2022 | "Disability hack": rotear áudio do Discord para captioner web | Frágil; o próprio blog saiu do ar |
| **Set/2022** | **OpenAI libera o Whisper** (~5–6% WER, robusto a ruído) | — o gargalo quebra — |
| 2023 | faster-whisper, whisper.cpp → transcrição local em tempo quase real em PC de gamer | |
| 2023–2026 | Onda de projetos: bots com faster-whisper, ARIA, Aurora, Seagull, overlays | Quase todos param no "postar num canal de texto" |
| Mar/2026 | **CaptionsRush** — primeiro produto completo (overlay + falantes) | Fechado, cloud pago, fraco em pt-BR |
| 2025–2026 | Modelos streaming *de verdade* com pt-BR: NVIDIA Nemotron-3.5 (jun/2026), AssemblyAI multilingual streaming (out/2025) | ← a janela em que estamos |

---

## 6. Arquitetura — as 3 rotas de captura e a vencedora

### Rota A — Bot no canal (streams separados por usuário)

Como funciona: `@discordjs/voice` (`VoiceReceiver.subscribe(userId)`) ou Pycord (`start_recording` + sinks) recebem **um stream Opus por usuário**, com identidade exata (SSRC → user_id via evento Speaking). Craig faz isso em produção desde 2017.

- ✅ Melhor qualidade possível: áudio isolado por falante → STT muito melhor, rótulos perfeitos
- ❌ **DAVE E2EE**: a criptografia ponta-a-ponta do Discord virou obrigatória em canais de voz em 01–02/mar/2026. discord.py quebrou totalmente (sem suporte); Pycord 2.7 conecta mas "gravação pode não funcionar como esperado"; @discordjs/voice tem bugs abertos de receive em canais DAVE. A implementação de referência (davey) é mantida por 1 voluntário (Snazzah — que também mantém o Craig).
- ❌ Voice receive é **API não documentada** desde sempre (tolerada, não garantida)
- ❌ Exige convidar bot ao servidor, hosting, e **cerimônia de consentimento** (Developer Policy exige permissão expressa para dados de voz; padrão do Craig: bot fica vermelho + anuncia gravação)

### Rota B — Áudio do sistema + diarização por IA

Como funciona: captura WASAPI do áudio do Discord (misturado) → diarização streaming (Diart ~500ms; NVIDIA Streaming Sortformer 4spk-v2, ago/2025, máx. 4 falantes; pyannoteAI Live-1 comercial <300ms) → identidade persistente via embeddings ECAPA-TDNN ("aprender os amigos" com ~30s de fala de cada um).

- ✅ Funciona com qualquer fonte de áudio (não só Discord)
- ❌ A parte mais frágil da cadeia: +0,5–1s de latência, erro alto com falas sobrepostas (o caso típico de gamer), teto de 4 falantes no Sortformer, e um segundo modelo neural brigando por GPU com o jogo e o STT
- **Veredito: só como fallback** para fontes não-Discord. Para o Discord, é usar IA para resolver um problema cuja resposta o Discord entrega pronta.

### Rota C — Discord RPC + captura por processo ✅ VENCEDORA

Como funciona: o **client desktop do Discord expõe uma API RPC local** (pipe IPC) com `GET_SELECTED_VOICE_CHANNEL`, eventos **`SPEAKING_START` / `SPEAKING_STOP` (com user_id de quem fala)** e `VOICE_STATE_*` (nomes, avatares, mute). Em paralelo, a **Process Loopback API** do Windows (Win10 2004+) captura **só o áudio do processo Discord.exe** — já descriptografado, então **imune ao DAVE** — sem pegar o áudio do jogo. O app alinha cada trecho transcrito com a janela de tempo de quem estava com o "anel verde" aceso.

- ✅ Rótulos exatos (nome, avatar, cor) sem bot, sem ML, sem hosting, sem setup no servidor
- ✅ Funciona em DM, grupo, qualquer servidor — o usuário não precisa de permissão de ninguém
- ✅ Prova de que funciona: **Overlayed** (overlay open-source Tauri que mostra quem fala, via RPC) e o próprio StreamKit do Discord
- ⚠️ Falas simultâneas viram um trecho com 2 candidatos → marcar "A + B" ou desambiguar com embedding leve (raro em prática)
- ⚠️ RPC para apps não aprovados é limitado ao time/testers do app → **distribuição em escala exige aprovação da Discord** (Overlayed conseguiu; para uso pessoal/beta não é bloqueio)
- ⚠️ Exige o client desktop aberto (gamer sempre tem)

**Estratégia: começar pela Rota C. Rota A vira um "modo alta fidelidade" opcional depois. Rota B vira fallback para capturar outras fontes.**

---

## 7. Stack recomendada

- **Shell + overlay:** **Tauri v2** (Rust) — janela `transparent + always_on_top + ignore_cursor_events`, config UI no mesmo app. Overlay **nunca injetado** em processo de jogo → zero exposição a anti-cheat (EAC/BattlEye/Vanguard). Borderless windowed é o padrão moderno (DX12 flip model + otimizações do Win11 praticamente aposentaram o fullscreen exclusivo); detectar FSE e orientar o usuário; segunda tela como fallback universal.
- **Captura:** WASAPI Process Loopback do Discord.exe (crate `wasapi`/`windows` em Rust; sample oficial da Microsoft é MIT) → ring buffer 16 kHz mono + Silero VAD.
- **Atribuição:** Discord RPC (`SPEAKING_START/STOP` + `VOICE_STATE_*`); alinhamento por timestamp; ECAPA-TDNN opcional só para sobreposição e "memória de amigos".
- **STT local (default):** **NVIDIA Nemotron-3.5-ASR-streaming-0.6b** (jun/2026) — streaming de verdade (latência configurável 80 ms–1,1 s), **inglês e pt-BR (5,48% WER)** no mesmo modelo, licença OpenMDW (comercial ok), runtime C++ (NeMo-Speech.cpp), roda em CPU int8 ou iGPU via DirectML → a dGPU fica 100% com o jogo. Fallback conservador: whisper.cpp small (Vulkan na iGPU).
- **STT cloud (opcional, para quem quiser máxima qualidade):** **AssemblyAI Universal-Streaming** — US$ 0,15/hora, ~300 ms, multilíngue com pt desde out/2025 → **~US$ 13,50/mês jogando 3h/dia** (Deepgram Nova-3: ~US$ 26–31/mês; Speechmatics ~US$ 36; Azure ~US$ 90+). Diarização cloud é desnecessária — o RPC já dá os falantes.
- **Custo de operação para você: ~zero** — tudo local por padrão; cloud é BYO-key ou assinatura do próprio usuário. Compatível com open-source.

### Comparativo STT local (resumo)

| Modelo | Streaming real? | pt-BR | Roda sem dGPU | Observação |
|---|---|---|---|---|
| **Nemotron-3.5-ASR-streaming-0.6b** | **Sim** (80ms–1,1s) | **Sim (5,48% WER)** | Sim (CPU int8/iGPU) | O achado da pesquisa; licença permissiva |
| whisper.cpp (stream) | Pseudo (janela deslizante) | Sim (small mediano, large bom) | Sim (Vulkan/CPU) | Fallback maduro e universal |
| faster-whisper | Não (chunks) | Melhor qualidade | CPU ok | Para modo "sem jogo rodando" |
| Moonshine streaming | Sim | **Não (só EN)** | Sim (CPU leve) | |
| Parakeet TDT v3 | Não (offline) | Sim | Sim | Rápido, mas sem streaming no sherpa-onnx |
| Vosk | Sim (parciais instantâneos) | Sim (modelo 50 MB) | Trivial | Precisão datada; bom p/ hardware fraco |

---

## 8. Open-source para reaproveitar

**Confirmação do seu palpite:** o "Wispr Flow grátis no GitHub" é quase certamente o **Handy** — e dá para reusar código de verdade (MIT).

| Projeto | Licença | O que reusar |
|---|---|---|
| **Handy** (cjpais) — ~29,6k ⭐ | MIT | **O blueprint do app**: Tauri v2 no Windows, captura cpal, Silero VAD, whisper.cpp + Parakeet via `transcribe-rs`, gestão de modelos, hotkeys globais. Ressalva: é ditado batch (aperta-fala-solta), não streaming — o streaming vem dos itens abaixo |
| **sherpa-onnx** (k2-fsa) — ~14k ⭐ | Apache-2.0 | Motor de **streaming ASR** com bindings Rust/Node, VAD, diarização — dá para embutir direto no Tauri |
| **RealtimeSTT** (KoljaB) — ~10k ⭐ | MIT | Caminho mais rápido para um protótipo: `feed_audio()` aceita PCM externo (nosso loopback) e emite parciais + finais |
| **WhisperLiveKit** — ~10,6k ⭐ | Apache-2.0 | O projeto OSS mais próximo do produto (SimulStreaming + diarização streaming embutida); referência de arquitetura |
| **Craig** (CraigChat) | ISC (=MIT) | Como fazer voice-receive por usuário em produção (para o futuro "modo bot") |
| **parrot-discord-transcriber** (jul/2026, 0 ⭐) | MIT | Protótipo do nosso produto menos o overlay: bot → PCM por usuário → legendas rolantes com faster-whisper. Ler de ponta a ponta |
| **discord-ext-voice-recv** | MIT | Voice receive para discord.py (autor é o mantenedor de voz do discord.py) |
| **electron-overlay-window** + Awakened PoE Trade | MIT | Como ancorar overlay sobre a janela do jogo (Electron); Awakened é um app de overlay completo em produção |
| **ecoute** | MIT | Padrão de captura loopback de speaker no Windows em Python |
| **asdf-overlay** | MIT/Apache-2.0 | Overlay por injeção DX (só se um dia precisar de fullscreen exclusivo — traz risco de anti-cheat) |

**Estudar sem copiar código (copyleft):** Overlayed (AGPL — a melhor referência de "overlay Tauri + RPC do Discord"), obs-localvocal (GPL-2 — melhor arquitetura de legendas contínuas com whisper.cpp), win-capture-audio (GPL-2 — usar o sample MIT da Microsoft no lugar). **Sem licença (não usar código):** dtinth/discord-transcriber (mas as notas de arquitetura são ótimas: debounce 0,5s/throttle 1,5s nas legendas).

Se o repositório que você viu não for o Handy, me manda o link que eu comparo com esses.

---

## 9. Riscos principais

1. **Fragilidade do lado Discord** — voice receive é não documentado; o DAVE E2EE quebrou bibliotecas inteiras em mar/2026; RPC já foi restringido antes e exige aprovação para distribuição em escala. *Mitigação:* Rota C depende só do client local (áudio pós-descriptografia); camada de ingestão isolada atrás de interface trocável; fallback de emergência com diarização leve.
2. **Contenção de GPU vs. qualidade em pt-BR** — os modelos que transcrevem português de gamer bem querem a GPU que o jogo está usando; modelos CPU pequenos degradam em gritaria sobreposta. *Mitigação:* Nemotron int8 em CPU/iGPU (benchmark obrigatório na Fase 2, com jogo real rodando); válvula de escape cloud barata.
3. **Anti-cheat e fullscreen exclusivo** — janela topmost não aparece em FSE e usuários de jogos com anti-cheat kernel (Vanguard) se assustam com overlays. *Mitigação:* nunca injetar; detectar modo de apresentação e orientar borderless; fallback segunda tela/celular.
4. **Risco existencial: Discord lançar nativo** — 6 anos ignorando pedidos e um overlay reformulado em mar/2025 sem captions sugerem que não é iminente. Se lançarem, o app ainda vence em customização, pt-BR e multi-fonte. E, francamente: se o Discord lançar, a missão de acessibilidade foi cumprida.
5. **Consentimento/privacidade** — mesmo sem bot, transcrever voz dos amigos pede transparência (indicador visível "legendas ativas"; nada de armazenar áudio por padrão; transcrever-e-descartar). Leis de gravação variam por país; no Brasil, gravar conversa da qual se participa é geralmente lícito. (Não é aconselhamento jurídico — checar na Fase 3.)

---

## 10. Plano — próximas fases

### Fase 2 — Validação (usuários + spikes técnicos)

**Com pessoas:**
- [ ] Testar o CaptionsRush a fundo (60–120 min grátis) — aprender com o que ele erra e acerta, especialmente em pt-BR
- [ ] Conversar com jogadores surdos/ensurdecidos: r/deafgamers, comunidades Discord de surdos (a da pesquisa da UFSM é um caminho — os autores mapearam a comunidade brasileira), a pessoa da palestra da EA se fizer sentido
- [ ] Validar requisitos: latência tolerável, aparência das legendas (diretrizes UFSM/BBC: fundo sólido, ~3 linhas), o que importa mais — nome? cor? avatar?

**Spikes técnicos (1–2 dias cada, ordem de risco):**
- [ ] **Spike 1 — o coração:** process loopback do Discord.exe + RPC `SPEAKING` events + Nemotron/whisper.cpp → medir latência ponta-a-ponta e acerto de atribuição numa call real com amigos
- [ ] **Spike 2 — overlay:** janela Tauri transparente click-through sobre um jogo real (borderless) com toggle por hotkey
- [ ] **Spike 3 — condições reais:** benchmark do STT em CPU média com jogo AAA rodando; comportamento com Valorant (Vanguard) e um jogo EAC
- [ ] **Spike 4 — RPC na prática:** limites do modo não-aprovado (testers), processo de aprovação da Discord (o Overlayed passou — há precedente)

### Fase 3 — Decisões de produto
- [ ] Escopo do MVP (chute inicial: Rota C + Nemotron local + overlay customizável + hotkey; bot mode e cloud ficam de fora do v1)
- [ ] Licença (MIT provável — todo o stack recomendado é compatível), nome, identidade
- [ ] Modelo de sustentabilidade (100% grátis local; cloud BYO-key — custo seu ~zero)
- [ ] Roadmap público + contato com Can I Play That / comunidade para lançamento

---

## Fontes principais

**Demanda / comunidade:** [Thread canônica de pedido (2020)](https://support.discord.com/hc/en-us/community/posts/360063450132) · [Thread em pt-BR](https://support.discord.com/hc/en-us/community/posts/14099277828247-Legendas-ao-vivo-durante-chamadas) · [Petição Change.org](https://www.change.org/p/discord-create-more-accessibility-on-discord) · [Pesquisa UFSM/RENOTE 2023 (PDF)](https://seer.ufrgs.br/renote/article/download/134338/89296) · [Buried Treasure — deaf gamers](https://buried-treasure.org/2021/04/falling-on-deaf-ears-how-games-need-to-be-more-accessible-to-deaf-players/) · [Thread Steam](https://steamcommunity.com/discussions/forum/0/3557193237102008185)

**Precedentes:** [Xbox Party Chat transcription](https://news.xbox.com/en-us/2021/05/12/introducing-party-chat-speech-transcription-and-synthesis/) · [PS5 accessibility](https://www.playstation.com/en-us/support/hardware/ps5-accessibility-settings/) · [Switch 2 GameChat](https://en.wikipedia.org/wiki/GameChat)

**Concorrência:** [CaptionsRush](https://captionsrush.com/) · [Review Can I Play That (mar/2026)](https://caniplaythat.com/2026/03/26/introducing-captionsrush-live-captioning-discord-voice-chat-while-gaming/) · [Scriptly](https://www.scriptly.xyz/) · [Scripty](https://scripty.org/) · [Windows Live Captions](https://support.microsoft.com/en-us/accessibility/windows/use-live-captions-to-better-understand-audio) · [Ava](https://www.ava.me/)

**Técnica:** [Discord RPC docs](https://docs.discord.com/developers/topics/rpc) · [Process Loopback sample (Microsoft)](https://learn.microsoft.com/en-us/samples/microsoft/windows-classic-samples/applicationloopbackaudio-sample/) · [DAVE E2EE enforcement (mar/2026)](https://support.discord.com/hc/en-us/articles/38749827197591-A-V-E2EE-Enforcement-for-Non-Stage-Voice-Calls) · [Nemotron-3.5-ASR-streaming](https://huggingface.co/nvidia/nemotron-3.5-asr-streaming-0.6b) · [AssemblyAI streaming pricing](https://www.assemblyai.com/pricing) · [Voice receive não documentado](https://github.com/discord/discord-api-docs/issues/808) · [Discord Developer Policy](https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy)

**Open-source:** [Handy](https://github.com/cjpais/Handy) · [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) · [RealtimeSTT](https://github.com/KoljaB/RealtimeSTT) · [WhisperLiveKit](https://github.com/QuentinFuxa/WhisperLiveKit) · [Craig](https://github.com/CraigChat/craig) · [parrot-discord-transcriber](https://github.com/chaosq3q/parrot-discord-transcriber) · [Overlayed](https://github.com/overlayeddev/overlayed) · [electron-overlay-window](https://github.com/SnosMe/electron-overlay-window) · [obs-localvocal](https://github.com/locaal-ai/obs-localvocal)
