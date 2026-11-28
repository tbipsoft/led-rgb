<p align="left">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/tbip-light.svg">
    <source media="(prefers-color-scheme: light)" srcset="docs/tbip-dark.svg">
    <img alt="Tbip" src="docs/tbip-dark.svg" width="120">
  </picture>
</p>

<p align="left"><strong>Turning Bits Into Products</strong></p>

# LED RGB

Troca as cores dos LEDs RGB do seu aparelho **na hora**, por uma tela simples controlada pelos
botões. Escolha uma cor ou um efeito, ajuste o brilho e pronto.

| RG52 Mini | R36S (sem LED compatível) |
|---|---|
| <img src="docs/previews/rg52-mini-led.png" alt="LED RGB no RG52 Mini" height="240"> | <img src="docs/previews/R36s-incompatible.png" alt="LED RGB no R36S, aparelho incompatível" height="240"> |

## 🎮 Aparelhos

Funciona no **RG52 Mini** e no **RG43 Pro** com dArkOS, os aparelhos que têm LEDs RGB controláveis.
Em qualquer outro aparelho o app abre, mas mostra **DEVICE INCOMPATIBLE** e não mexe em nada.

## 📥 Como instalar

1. Abra a página da [última versão](https://github.com/tbipsoft/led-rgb/releases/latest)
   (no GitHub, é o link **Releases** na coluna da direita).
2. Em **Assets**, baixe o arquivo `led-rgb.tar.gz` e extraia. Dentro há três arquivos:
   - `led-rgb`: o programa;
   - `LED RGB.sh`: o script que abre o programa pelo menu;
   - `led-rgbctl`: ferramenta para desenvolvedores (opcional, o menu não precisa dela).
3. Coloque o cartão microSD no computador. Na raiz dele, abra a pasta `roms/tools`, crie
   dentro dela uma pasta `Tbip` (se ainda não existir) e copie os arquivos para lá.
4. Devolva o cartão ao aparelho e ligue. Abra o menu do sistema (geralmente pelo botão
   **Start**) e vá em **Options → Tools**. Se deu tudo certo, aparece a pasta **Tbip** com o
   **LED RGB** dentro.

## 🕹️ Como usar

| Botão | O que faz |
|---|---|
| Direcional | escolhe a cor ou o efeito; **cada movimento já troca a cor** (com os LEDs ligados) |
| **A** | liga e desliga os LEDs |
| **L1 / R1** | diminui / aumenta o brilho |
| **B** | sai (os LEDs ficam como estão) |

Também dá para tocar na tela, num quadro de cor, no interruptor ou na barra de brilho.

Há cores fixas, combinações e efeitos como **rainbow** (arco-íris), **scroll** e **breathing**
("respirando"). A prévia na tela é aproximada: os efeitos animados acontecem nos próprios LEDs.

A cor escolhida fica salva e é a mesma usada pelo **LED Settings** do dArkOS. Só não use os dois
ao mesmo tempo, porque eles disputam o controle dos LEDs.

## 💬 Encontrou um problema ou tem uma sugestão?

Se algo não funcionou no seu aparelho ou você tem uma ideia para melhorar o app,
[abra uma issue](https://github.com/tbipsoft/led-rgb/issues) ou mande um e-mail para
[tbipsoft@gmail.com](mailto:tbipsoft@gmail.com). Ao relatar uma falha, diga o
modelo do aparelho, a versão do app e, se possível, o que aparece em `/tmp/led-rgb.log`.

Também aceitamos sugestões de **novos apps**: conte o que você gostaria de ter no seu aparelho.

---

Ferramentas open source para consoles portáteis com dArkOS

A **Tbip** (*Turning Bits Into Products*, ou "transformando bits em produtos") desenvolve ferramentas **open source** para o [dArkOS](https://github.com/christianhaitian/dArkOS), sistema usado em consoles portáteis de jogos. Nosso objetivo é substituir telas de terminal e scripts por interfaces gráficas claras, feitas para usar com os botões do próprio aparelho.

Todas as ferramentas são escritas em **Rust** e usam **SDL2**. Elas identificam o aparelho automaticamente, mostram apenas o que ele possui e adaptam o layout à tela.

## 🔓 Open source

Nossos projetos são distribuídos sob a **licença MIT**. Você pode usar, estudar, modificar e redistribuir o código, inclusive em outros projetos, desde que mantenha o aviso de copyright e a licença.

🛠️ Quer compilar, testar ou fazer um fork? O [guia de desenvolvimento](docs/desenvolvimento.md) explica como o app funciona por dentro.
