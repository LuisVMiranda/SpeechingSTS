use serde::{Deserialize, Serialize};

/// UI language is deliberately separate from the language sent to the model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiLanguage {
    Spanish,
    PortugueseBrazil,
    #[default]
    #[serde(other)]
    English,
}

impl UiLanguage {
    pub const ALL: [Self; 3] = [Self::English, Self::Spanish, Self::PortugueseBrazil];
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Español",
            Self::PortugueseBrazil => "Português (Brasil)",
        }
    }
    pub fn text<'a>(self, english: &'a str) -> &'a str {
        if self == Self::English {
            return english;
        }
        CATALOG
            .iter()
            .find(|entry| entry.0 == english)
            .map(|entry| {
                if self == Self::Spanish {
                    entry.1
                } else {
                    entry.2
                }
            })
            .unwrap_or(english)
    }
    pub fn format(self, template: &str, value: &str) -> String {
        self.text(template).replace("{0}", value)
    }
    /// Translate app-owned errors but retain diagnostic details and provider text.
    pub fn message(self, message: &str) -> String {
        let exact = self.text(message);
        if exact != message || self == Self::English {
            return exact.to_owned();
        }
        if let Some(count) = message.strip_suffix(" conversation file(s) could not be read.") {
            let key = if count == "1" {
                "{0} conversation file could not be read."
            } else {
                "{0} conversation file(s) could not be read."
            };
            return self.format(key, count);
        }
        for (prefix, key) in ERROR_PREFIXES {
            if let Some(detail) = message.strip_prefix(prefix) {
                return format!("{}{}", self.text(key), self.message(detail));
            }
        }
        message.to_owned()
    }
}

// Each row is English, Spanish and Brazilian Portuguese. Original model text,
// user-authored instructions and provider errors are never translated here.
const CATALOG: &[(&str, &str, &str)] = &[
    ("Interface language", "Idioma de la aplicación", "Idioma do aplicativo"),
    ("Speeching settings", "Configuración de Speeching", "Configurações do Speeching"),
    ("Speeching conversation log", "Historial de Speeching", "Histórico do Speeching"),
    ("Speeching bubbles", "Burbujas de Speeching", "Balões do Speeching"),
    ("Settings", "Configuración", "Configurações"),
    ("Close settings", "Cerrar configuración", "Fechar configurações"),
    ("Open settings", "Abrir configuración", "Abrir configurações"),
    ("Conversation log", "Historial de conversación", "Histórico da conversa"),
    ("Close conversation log", "Cerrar historial", "Fechar histórico"),
    ("Open conversation log", "Abrir historial", "Abrir histórico"),
    ("Toggle conversation text", "Mostrar u ocultar texto", "Mostrar ou ocultar texto"),
    ("Show bubbles", "Mostrar burbujas", "Mostrar balões"),
    ("Hide conversation text", "Ocultar texto de la conversación", "Ocultar texto da conversa"),
    ("Toggle microphone", "Activar o desactivar micrófono", "Ativar ou desativar microfone"),
    ("Unmute microphone", "Activar micrófono", "Ativar microfone"),
    ("Mute microphone", "Silenciar micrófono", "Silenciar microfone"),
    ("End conversation", "Terminar conversación", "Encerrar conversa"),
    ("Start conversation", "Iniciar conversación", "Iniciar conversa"),
    ("Practice language", "Idioma de práctica", "Idioma de prática"),
    ("Choose a supported language", "Elige un idioma compatible", "Escolha um idioma compatível"),
    ("Search supported languages", "Buscar idiomas compatibles", "Buscar idiomas compatíveis"),
    ("No matches. Try another language.", "Sin resultados. Prueba otro idioma.", "Nenhum resultado. Tente outro idioma."),
    ("Save settings", "Guardar configuración", "Salvar configurações"),
    ("Make the conversation your own.", "Personaliza tu conversación.", "Personalize sua conversa."),
    ("Appearance", "Apariencia", "Aparência"),
    ("Light", "Claro", "Claro"),
    ("Dark", "Oscuro", "Escuro"),
    ("Practice", "Práctica", "Prática"),
    ("Level", "Nivel", "Nível"),
    ("Beginner", "Principiante", "Iniciante"),
    ("Intermediate", "Intermedio", "Intermediário"),
    ("Advanced", "Avanzado", "Avançado"),
    ("Speaking pace", "Velocidad del habla", "Velocidade da fala"),
    ("Very slow", "Muy lento", "Muito lenta"),
    ("A little slower", "Un poco más lento", "Um pouco mais lenta"),
    ("Natural", "Natural", "Natural"),
    ("A little faster", "Un poco más rápido", "Um pouco mais rápida"),
    ("Fast", "Rápido", "Rápida"),
    ("Adjusts the partner's speech while preserving voice pitch. Applies to the next conversation.", "Ajusta la velocidad sin cambiar el tono de voz. Se aplica a la próxima conversación.", "Ajusta a velocidade sem alterar o tom de voz. Vale para a próxima conversa."),
    ("Your instructions", "Tus instrucciones", "Suas instruções"),
    ("e.g. Correct my mistakes gently and ask about everyday life", "Ej.: Corrige mis errores con amabilidad y pregunta sobre mi día", "Ex.: Corrija meus erros com gentileza e pergunte sobre meu dia"),
    ("Added to the partner's system instructions for the next conversation.", "Se añaden a las instrucciones del compañero en la próxima conversación.", "São adicionadas às instruções do parceiro na próxima conversa."),
    ("Microphone", "Micrófono", "Microfone"),
    ("Voice activated", "Activación por voz", "Ativação por voz"),
    ("Push to talk", "Pulsar para hablar", "Pressionar para falar"),
    ("Shortcut", "Atajo", "Atalho"),
    ("Press a key combination…", "Pulsa una combinación de teclas…", "Pressione uma combinação de teclas…"),
    ("Press a key with optional Ctrl, Alt or Shift. Esc cancels.", "Pulsa una tecla con Ctrl, Alt o Mayús si quieres. Esc cancela.", "Pressione uma tecla com Ctrl, Alt ou Shift se quiser. Esc cancela."),
    ("Hold this shortcut while the main window is focused; release to send.", "Mantén pulsado el atajo con la ventana principal activa; suéltalo para enviar.", "Segure o atalho com a janela principal ativa; solte para enviar."),
    ("Speech is detected automatically.", "La voz se detecta automáticamente.", "A fala é detectada automaticamente."),
    ("Reduce quiet background noise", "Reducir ruido de fondo tenue", "Reduzir ruídos baixos de fundo"),
    ("Noise threshold", "Umbral de ruido", "Limite de ruído"),
    ("Raise the threshold for a noisier room; too high can suppress quiet speech.", "Aumenta el umbral en un lugar ruidoso; si es demasiado alto puede silenciar voces suaves.", "Aumente o limite em ambientes ruidosos; um valor alto pode cortar falas baixas."),
    ("Bubble lifetime", "Duración de las burbujas", "Duração dos balões"),
    ("Reset to 8 s", "Restablecer a 8 s", "Restaurar para 8 s"),
    ("Provider", "Proveedor", "Provedor"),
    ("WebSocket URL", "URL de WebSocket", "URL do WebSocket"),
    ("Model", "Modelo", "Modelo"),
    ("API key", "Clave de API", "Chave de API"),
    ("Paste API key (not saved)", "Pega la clave de API (no se guarda)", "Cole a chave de API (não é salva)"),
    ("Voice", "Voz", "Voz"),
    ("Voice symbols describe perceived register; individual perception can vary.", "Los símbolos describen el registro percibido; cada persona puede oírlo de forma distinta.", "Os símbolos descrevem o registro percebido; a percepção pode variar."),
    ("Your key is sent to this URL and kept only in memory. Custom endpoints must use the selected provider's live protocol.", "La clave se envía a esta URL y solo se guarda en memoria. Los servidores personalizados deben usar el protocolo del proveedor elegido.", "A chave é enviada a esta URL e fica apenas na memória. Endereços personalizados devem usar o protocolo do provedor escolhido."),
    ("Provider, language, voice and pace changes apply to the next conversation.", "Los cambios de proveedor, idioma, voz y velocidad se aplican a la próxima conversación.", "Alterações de provedor, idioma, voz e velocidade valem para a próxima conversa."),
    ("Conversations are saved locally on this computer.", "Las conversaciones se guardan localmente en este equipo.", "As conversas são salvas localmente neste computador."),
    ("New conversation", "Nueva conversación", "Nova conversa"),
    ("Close saved chats", "Cerrar conversaciones guardadas", "Fechar conversas salvas"),
    ("Saved chats…", "Conversaciones guardadas…", "Conversas salvas…"),
    ("Search saved conversations", "Buscar conversaciones guardadas", "Buscar conversas salvas"),
    ("No conversations found.", "No se encontraron conversaciones.", "Nenhuma conversa encontrada."),
    ("Selected: {0}", "Seleccionada: {0}", "Selecionada: {0}"),
    ("Recent messages will be sent; older messages remain in the log.", "Se enviarán los mensajes recientes; los anteriores permanecerán en el historial.", "As mensagens recentes serão enviadas; as anteriores permanecem no histórico."),
    ("Previous messages will be sent to your provider when you start.", "Al iniciar, los mensajes anteriores se enviarán al proveedor.", "Ao iniciar, as mensagens anteriores serão enviadas ao provedor."),
    ("New conversation · choose a saved chat to continue it.", "Nueva conversación · elige una conversación guardada para continuarla.", "Nova conversa · escolha uma conversa salva para continuá-la."),
    ("Copy", "Copiar", "Copiar"),
    ("Download", "Descargar", "Baixar"),
    ("Clear", "Borrar", "Limpar"),
    ("Text file", "Archivo de texto", "Arquivo de texto"),
    ("Your conversation log will appear here.", "El historial aparecerá aquí.", "O histórico da conversa aparecerá aqui."),
    ("Conversation", "Conversación", "Conversa"),
    ("You", "Tú", "Você"),
    ("Microphone muted", "Micrófono silenciado", "Microfone silenciado"),
    ("Release {0} to send", "Suelta {0} para enviar", "Solte {0} para enviar"),
    ("Hold {0} to speak", "Mantén pulsado {0} para hablar", "Segure {0} para falar"),
    ("Speaking…", "Hablando…", "Falando…"),
    ("Listening…", "Escuchando…", "Ouvindo…"),
    ("Ready when you are", "Cuando quieras", "Quando você quiser"),
    ("Choose a supported language in settings.", "Elige un idioma compatible en configuración.", "Escolha um idioma compatível nas configurações."),
    ("Add your API key in settings.", "Añade tu clave de API en configuración.", "Adicione sua chave de API nas configurações."),
    ("Choose a language, model, and voice in settings.", "Elige un idioma, modelo y voz en configuración.", "Escolha idioma, modelo e voz nas configurações."),
    ("Connecting…", "Conectando…", "Conectando…"),
    ("Reconnecting…", "Reconectando…", "Reconectando…"),
    ("Ending conversation…", "Terminando conversación…", "Encerrando conversa…"),
    ("Listening when you are ready", "Escuchando cuando estés listo", "Ouvindo quando você estiver pronto"),
    ("Conversation ended", "Conversación terminada", "Conversa encerrada"),
    ("Conversation copied", "Conversación copiada", "Conversa copiada"),
    ("Copied to clipboard", "Copiado al portapapeles", "Copiado para a área de transferência"),
    ("Conversation saved", "Conversación guardada", "Conversa salva"),
    ("Could not save the conversation", "No se pudo guardar la conversación", "Não foi possível salvar a conversa"),
    ("Conversation log cleared", "Historial borrado", "Histórico limpo"),
    ("Conversation cleared", "Conversación borrada", "Conversa limpa"),
    ("Ready for a new conversation", "Todo listo para una nueva conversación", "Tudo pronto para uma nova conversa"),
    ("Selected. Start the microphone to continue.", "Seleccionada. Inicia el micrófono para continuar.", "Selecionada. Inicie o microfone para continuar."),
    ("Ready to continue", "Listo para continuar", "Tudo pronto para continuar"),
    ("Just now", "Ahora mismo", "Agora mesmo"),
    ("{0} min ago", "Hace {0} min", "Há {0} min"),
    ("{0} h ago", "Hace {0} h", "Há {0} h"),
    ("{0} d ago", "Hace {0} días", "Há {0} dias"),
    ("1 day ago", "Hace 1 día", "Há 1 dia"),
    ("{0} conversation file could not be read.", "No se pudo leer {0} archivo de conversación.", "Não foi possível ler {0} arquivo de conversa."),
    ("{0} conversation file(s) could not be read.", "No se pudieron leer {0} archivos de conversación.", "Não foi possível ler {0} arquivos de conversa."),
    ("Space", "Espacio", "Espaço"),
    ("Shift", "Mayús", "Shift"), ("Enter", "Intro", "Enter"),
    ("Tab", "Tabulador", "Tab"), ("Backspace", "Retroceso", "Backspace"),
    ("Delete", "Suprimir", "Delete"), ("Home", "Inicio", "Home"),
    ("End", "Fin", "End"), ("ArrowUp", "Flecha arriba", "Seta para cima"),
    ("ArrowDown", "Flecha abajo", "Seta para baixo"), ("ArrowLeft", "Flecha izquierda", "Seta para a esquerda"),
    ("ArrowRight", "Flecha derecha", "Seta para a direita"),
    ("Afrikaans", "Afrikáans", "Africâner"), ("Akan", "Acano", "Akan"),
    ("Albanian", "Albanés", "Albanês"), ("Amharic", "Amárico", "Amárico"),
    ("Arabic", "Árabe", "Árabe"), ("Armenian", "Armenio", "Armênio"),
    ("Assamese", "Asamés", "Assamês"), ("Azerbaijani", "Azerbaiyano", "Azerbaijano"),
    ("Basque", "Euskera", "Basco"), ("Belarusian", "Bielorruso", "Bielorrusso"),
    ("Bengali", "Bengalí", "Bengali"), ("Bosnian", "Bosnio", "Bósnio"),
    ("Bulgarian", "Búlgaro", "Búlgaro"), ("Burmese", "Birmano", "Birmanês"),
    ("Catalan", "Catalán", "Catalão"), ("Cebuano", "Cebuano", "Cebuano"),
    ("Chinese", "Chino", "Chinês"), ("Chinese (Simplified)", "Chino (simplificado)", "Chinês (simplificado)"),
    ("Chinese (Traditional)", "Chino (tradicional)", "Chinês (tradicional)"), ("Croatian", "Croata", "Croata"),
    ("Czech", "Checo", "Tcheco"), ("Danish", "Danés", "Dinamarquês"),
    ("Dutch", "Neerlandés", "Holandês"), ("English", "Inglés", "Inglês"),
    ("Estonian", "Estonio", "Estoniano"), ("Faroese", "Feroés", "Feroês"),
    ("Filipino", "Filipino", "Filipino"), ("Finnish", "Finés", "Finlandês"),
    ("French", "Francés", "Francês"), ("Galician", "Gallego", "Galego"),
    ("Georgian", "Georgiano", "Georgiano"), ("German", "Alemán", "Alemão"),
    ("Greek", "Griego", "Grego"), ("Gujarati", "Guyaratí", "Guzerate"),
    ("Hausa", "Hausa", "Hauçá"), ("Hebrew", "Hebreo", "Hebraico"),
    ("Hindi", "Hindi", "Hindi"), ("Hungarian", "Húngaro", "Húngaro"),
    ("Icelandic", "Islandés", "Islandês"), ("Indonesian", "Indonesio", "Indonésio"),
    ("Irish", "Irlandés", "Irlandês"), ("Italian", "Italiano", "Italiano"),
    ("Japanese", "Japonés", "Japonês"), ("Kannada", "Canarés", "Canarês"),
    ("Kazakh", "Kazajo", "Cazaque"), ("Khmer", "Jemer", "Khmer"),
    ("Kinyarwanda", "Kinyarwanda", "Quiniaruanda"), ("Korean", "Coreano", "Coreano"),
    ("Kurdish", "Kurdo", "Curdo"), ("Kyrgyz", "Kirguís", "Quirguiz"),
    ("Lao", "Lao", "Laosiano"), ("Latvian", "Letón", "Letão"),
    ("Lithuanian", "Lituano", "Lituano"), ("Macedonian", "Macedonio", "Macedônio"),
    ("Malay", "Malayo", "Malaio"), ("Malayalam", "Malayalam", "Malaiala"),
    ("Maltese", "Maltés", "Maltês"), ("Maori", "Maorí", "Maori"),
    ("Marathi", "Maratí", "Marata"), ("Mongolian", "Mongol", "Mongol"),
    ("Nepali", "Nepalí", "Nepalês"), ("Norwegian", "Noruego", "Norueguês"),
    ("Odia", "Odia", "Oriá"), ("Oromo", "Oromo", "Oromo"),
    ("Pashto", "Pastún", "Pashto"), ("Persian", "Persa", "Persa"),
    ("Polish", "Polaco", "Polonês"), ("Portuguese", "Portugués", "Português"),
    ("Portuguese (Brazil)", "Portugués (Brasil)", "Português (Brasil)"),
    ("Portuguese (Portugal)", "Portugués (Portugal)", "Português (Portugal)"),
    ("Punjabi", "Panyabí", "Punjabi"), ("Quechua", "Quechua", "Quíchua"),
    ("Romanian", "Rumano", "Romeno"), ("Romansh", "Romanche", "Romanche"),
    ("Russian", "Ruso", "Russo"), ("Serbian", "Serbio", "Sérvio"),
    ("Sindhi", "Sindhi", "Sindi"), ("Sinhala", "Cingalés", "Cingalês"),
    ("Slovak", "Eslovaco", "Eslovaco"), ("Slovenian", "Esloveno", "Esloveno"),
    ("Somali", "Somalí", "Somali"), ("Southern Sotho", "Sesoto meridional", "Soto do sul"),
    ("Spanish", "Español", "Espanhol"), ("Swahili", "Suajili", "Suaíli"),
    ("Swedish", "Sueco", "Sueco"), ("Tagalog", "Tagalo", "Tagalo"),
    ("Tajik", "Tayiko", "Tadjique"), ("Tamil", "Tamil", "Tâmil"),
    ("Telugu", "Telugu", "Telugu"), ("Thai", "Tailandés", "Tailandês"),
    ("Tswana", "Setsuana", "Tswana"), ("Turkish", "Turco", "Turco"),
    ("Turkmen", "Turcomano", "Turcomeno"), ("Ukrainian", "Ucraniano", "Ucraniano"),
    ("Urdu", "Urdu", "Urdu"), ("Uzbek", "Uzbeko", "Uzbeque"),
    ("Vietnamese", "Vietnamita", "Vietnamita"), ("Welsh", "Galés", "Galês"),
    ("Western Frisian", "Frisón occidental", "Frísio ocidental"), ("Wolof", "Wólof", "Uolofe"),
    ("Yoruba", "Yoruba", "Iorubá"), ("Zulu", "Zulú", "Zulu"),
    ("Breezy", "Desenfadada", "Descontraída"), ("Upbeat", "Animada", "Animada"),
    ("Firm", "Firme", "Firme"), ("Bright", "Vivaz", "Vibrante"),
    ("Informative", "Informativa", "Informativa"), ("Excitable", "Entusiasta", "Empolgada"),
    ("Youthful", "Juvenil", "Jovem"), ("Easy-going", "Tranquila", "Descontraída"),
    ("Breathy", "Aireada", "Soprosa"), ("Clear voice", "Clara", "Clara"),
    ("Smooth", "Suave", "Suave"), ("Gravelly", "Rasposa", "Rouxca"),
    ("Even", "Uniforme", "Equilibrada"), ("Mature", "Madura", "Madura"),
    ("Forward", "Directa", "Direta"), ("Friendly", "Amable", "Amigável"),
    ("Casual", "Informal", "Informal"), ("Gentle", "Dulce", "Gentil"),
    ("Lively", "Animada", "Animada"), ("Knowledgeable", "Conocedora", "Experiente"),
    ("Warm", "Cálida", "Acolhedora"), ("Soft", "Suave", "Suave"),
    ("Could not start the audio engine.", "No se pudo iniciar el motor de audio.", "Não foi possível iniciar o mecanismo de áudio."),
    ("No microphone was found.", "No se encontró ningún micrófono.", "Nenhum microfone foi encontrado."),
    ("No audio output device was found.", "No se encontró ningún dispositivo de salida de audio.", "Nenhum dispositivo de saída de áudio foi encontrado."),
    ("Invalid provider URL.", "La URL del proveedor no es válida.", "A URL do provedor é inválida."),
    ("Invalid API key.", "La clave de API no es válida.", "A chave de API é inválida."),
    ("The provider reported an error.", "El proveedor informó de un error.", "O provedor informou um erro."),
    ("Enter a valid WebSocket URL.", "Introduce una URL de WebSocket válida.", "Informe uma URL de WebSocket válida."),
    ("Use wss:// for a remote provider, or ws:// for a local server.", "Usa wss:// para un proveedor remoto o ws:// para un servidor local.", "Use wss:// para um provedor remoto ou ws:// para um servidor local."),
    ("Remove credentials from the URL; use the API key field instead.", "Quita las credenciales de la URL y usa el campo de clave de API.", "Remova credenciais da URL e use o campo da chave de API."),
    ("Could not connect. Check the provider URL, key, model access, and network.", "No se pudo conectar. Comprueba la URL, la clave, el acceso al modelo y la red.", "Não foi possível conectar. Verifique a URL, a chave, o acesso ao modelo e a rede."),
    ("Could not configure the live session.", "No se pudo configurar la sesión en vivo.", "Não foi possível configurar a sessão ao vivo."),
    ("The provider did not finish setting up the session. Check model access and try again.", "El proveedor no terminó de configurar la sesión. Comprueba el acceso al modelo e inténtalo de nuevo.", "O provedor não concluiu a configuração. Verifique o acesso ao modelo e tente novamente."),
    ("Could not restore the selected conversation.", "No se pudo recuperar la conversación seleccionada.", "Não foi possível restaurar a conversa selecionada."),
    ("The conversation disconnected. Check your connection or quota and try again.", "La conversación se desconectó. Comprueba tu conexión o cuota e inténtalo de nuevo.", "A conversa foi desconectada. Verifique a conexão ou a cota e tente novamente."),
    ("Choose a supported language from the suggestions.", "Elige un idioma compatible de las sugerencias.", "Escolha um idioma compatível nas sugestões."),
    ("Could not locate the settings folder.", "No se encontró la carpeta de configuración.", "Não foi possível localizar a pasta de configurações."),
    ("Invalid settings path.", "La ruta de configuración no es válida.", "O caminho das configurações é inválido."),
    ("Invalid conversation ID.", "El ID de conversación no es válido.", "O ID da conversa é inválido."),
    ("Unsupported conversation file.", "El archivo de conversación no es compatible.", "O arquivo da conversa não é compatível."),
    ("Could not save settings: ", "No se pudo guardar la configuración: ", "Não foi possível salvar as configurações: "),
    ("Could not save window positions: ", "No se pudieron guardar las posiciones de las ventanas: ", "Não foi possível salvar as posições das janelas: "),
    ("Cannot create the conversations folder: ", "No se pudo crear la carpeta de conversaciones: ", "Não foi possível criar a pasta de conversas: "),
    ("Could not save conversation: ", "No se pudo guardar la conversación: ", "Não foi possível salvar a conversa: "),
    ("Could not finish saving conversation: ", "No se pudo terminar de guardar la conversación: ", "Não foi possível concluir o salvamento da conversa: "),
    ("Could not open conversation: ", "No se pudo abrir la conversación: ", "Não foi possível abrir a conversa: "),
    ("Could not read conversation: ", "No se pudo leer la conversación: ", "Não foi possível ler a conversa: "),
    ("Microphone: ", "Micrófono: ", "Microfone: "),
    ("Audio output: ", "Salida de audio: ", "Saída de áudio: "),
    ("Unsupported microphone format: ", "Formato de micrófono no compatible: ", "Formato de microfone não compatível: "),
    ("Unsupported output format: ", "Formato de salida no compatible: ", "Formato de saída não compatível: "),
    ("Could not start microphone: ", "No se pudo iniciar el micrófono: ", "Não foi possível iniciar o microfone: "),
    ("Could not start audio output: ", "No se pudo iniciar la salida de audio: ", "Não foi possível iniciar a saída de áudio: "),
    ("Could not open microphone: ", "No se pudo abrir el micrófono: ", "Não foi possível abrir o microfone: "),
    ("Could not open audio output: ", "No se pudo abrir la salida de audio: ", "Não foi possível abrir a saída de áudio: "),
];

const ERROR_PREFIXES: &[(&str, &str)] = &[
    ("Could not save settings: ", "Could not save settings: "),
    (
        "Could not save window positions: ",
        "Could not save window positions: ",
    ),
    (
        "Cannot create the conversations folder: ",
        "Cannot create the conversations folder: ",
    ),
    (
        "Could not save conversation: ",
        "Could not save conversation: ",
    ),
    (
        "Could not finish saving conversation: ",
        "Could not finish saving conversation: ",
    ),
    (
        "Could not open conversation: ",
        "Could not open conversation: ",
    ),
    (
        "Could not read conversation: ",
        "Could not read conversation: ",
    ),
    ("Microphone: ", "Microphone: "),
    ("Audio output: ", "Audio output: "),
    (
        "Unsupported microphone format: ",
        "Unsupported microphone format: ",
    ),
    ("Unsupported output format: ", "Unsupported output format: "),
    (
        "Could not start microphone: ",
        "Could not start microphone: ",
    ),
    (
        "Could not start audio output: ",
        "Could not start audio output: ",
    ),
    ("Could not open microphone: ", "Could not open microphone: "),
    (
        "Could not open audio output: ",
        "Could not open audio output: ",
    ),
];

#[cfg(test)]
pub fn has_translation(key: &str) -> bool {
    CATALOG.iter().any(|entry| entry.0 == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_catalog_entries_are_unique_and_have_both_translations() {
        let mut names = std::collections::HashSet::new();
        for (en, es, pt) in CATALOG {
            assert!(names.insert(en), "duplicate translation: {en}");
            assert!(
                !es.is_empty() && !pt.is_empty(),
                "missing translation: {en}"
            );
            assert_eq!(en.matches("{0}").count(), es.matches("{0}").count());
            assert_eq!(en.matches("{0}").count(), pt.matches("{0}").count());
        }
    }
    #[test]
    fn unknown_provider_text_and_error_details_are_preserved() {
        assert_eq!(
            UiLanguage::Spanish.message("provider custom error"),
            "provider custom error"
        );
        assert_eq!(
            UiLanguage::PortugueseBrazil.message("Could not save conversation: disk full"),
            "Não foi possível salvar a conversa: disk full"
        );
    }
}
