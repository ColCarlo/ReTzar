//! The English of ReTzar's own texts, those the original game does not have:
//! the pages and hints it adds, its notices and its errors. Each pair is
//! the Italian text, exactly as written in the code, and its translation in
//! the American English of the original's release. `{}` stands for what the
//! code puts in; `{a}` and `{b}` are the two kingdoms of the cheat panel's
//! diplomacy page.
pub const ENGLISH: &[(&str, &str)] = &[
    // The settings (`menu/settings.rs`): tabs, groups and rows.
    ("Gioco", "Game"),
    ("Schermo", "Display"),
    ("Grafica", "Graphics"),
    ("Tasti", "Keys"),
    ("PARTITA", "GAME"),
    ("MAPPA", "MAP"),
    ("GIOCATORE", "PLAYER"),
    ("FINESTRA", "WINDOW"),
    ("INTERFACCIA", "INTERFACE"),
    ("IMMAGINE", "PICTURE"),
    ("VISUALE", "VIEW"),
    ("EVENTI E MAPPA", "EVENTS AND MAP"),
    ("FINESTRE", "WINDOWS"),
    ("Effetti sonori", "Sound effects"),
    ("Musica", "Music"),
    ("Voci delle unità", "Unit voices"),
    ("Suoni degli edifici", "Building sounds"),
    ("Suoni della natura", "Nature sounds"),
    ("Velocità del gioco", "Game speed"),
    ("Clima", "Weather"),
    ("Giorno e notte", "Day and night"),
    ("Velocità di scorrimento", "Scroll speed"),
    ("Scorrimento ai bordi", "Edge scrolling"),
    ("Nome sulle proprie unità", "Name over your units"),
    ("Lingua", "Language"),
    (
        "Ricorda le missioni raggiunte",
        "Remember the missions reached",
    ),
    ("Schermo intero", "Full screen"),
    ("Dimensioni della finestra", "Window size"),
    ("Fotogrammi al secondo", "Frames per second"),
    ("Scala", "Scale"),
    ("Pannello dei comandi", "Command panel"),
    ("Puntatore", "Pointer"),
    ("Testi sulla mappa", "Texts on the map"),
    ("Griglia di piazzamento", "Placement grid"),
    ("Colori a 24 bit", "24-bit colors"),
    ("Pixel della mappa", "Map pixels"),
    ("Effetto monitor CRT", "CRT monitor effect"),
    ("Testi in alta definizione", "High-definition texts"),
    ("Luci notturne", "Night lights"),
    ("Bagliore di fuochi e magie", "Glow of fire and magic"),
    ("Nebbia morbida", "Soft fog"),
    ("Ambiente vivo", "Living world"),
    ("Zoom fluido", "Smooth zoom"),
    // Their choices and values.
    ("Pixel nitidi", "Sharp pixels"),
    ("Adatta allo schermo", "Fit to screen"),
    ("A destra", "On the right"),
    ("A sinistra", "On the left"),
    ("Del gioco", "The game's"),
    ("Di Windows", "Windows'"),
    ("Originali", "Original"),
    ("Contorno", "Outline"),
    ("Fondo scuro", "Dark band"),
    ("Leggera", "Faint"),
    ("Media", "Medium"),
    ("Marcata", "Strong"),
    ("Regolari", "Even"),
    ("Levigati", "Smoothed"),
    ("Sincronizzazione verticale", "Vertical sync"),
    ("nessuno", "none"),
    ("Sì", "Yes"),
    // Their buttons and lines.
    ("Predefiniti", "Defaults"),
    ("Annulla", "Cancel"),
    (
        "Scrivi il numero e premi Invio; 0 toglie il limite, Esc annulla.",
        "Type the number and press Enter; 0 removes the limit, Esc cancels.",
    ),
    ("Premi un tasto…", "Press a key…"),
    (
        "Premi il nuovo tasto; Esc annulla.",
        "Press the new key; Esc cancels.",
    ),
    (
        "Aggiunta di questa versione, assente nell'originale.",
        "An addition of this version, not in the original.",
    ),
    // What each setting does.
    (
        "Volumi, voci e suoni del gioco.",
        "Volumes, voices and sounds of the game.",
    ),
    (
        "La partita, lo scorrimento della mappa, il tuo nome e la lingua.",
        "The game, the scrolling of the map, your name and the language.",
    ),
    (
        "La finestra del gioco e l'aspetto dell'interfaccia.",
        "The game's window and the look of the interface.",
    ),
    (
        "Gli effetti grafici moderni, tutti facoltativi e spenti all'inizio.",
        "The modern graphics effects, all optional and off at first.",
    ),
    (
        "I tasti dei comandi sulla mappa.",
        "The keys of the map commands.",
    ),
    (
        "Effetti, voci e rumori della mappa: l'interruttore li spegne, il cursore ne regola il volume.",
        "Effects, voices and noises of the map: the switch turns them off, the slider sets their volume.",
    ),
    (
        "La musica di menu e missioni: l'interruttore la spegne, il cursore ne regola il volume.",
        "The music of menus and missions: the switch turns it off, the slider sets its volume.",
    ),
    (
        "Le risposte delle unità quando le selezioni e dai loro ordini.",
        "The units' replies when you select them and give them orders.",
    ),
    (
        "Il suono degli edifici quando li selezioni.",
        "The sound of buildings when you select them.",
    ),
    (
        "Animali, vento e pioggia intorno alla parte di mappa che guardi.",
        "Animals, wind and rain around the part of the map you are looking at.",
    ),
    (
        "Quanto scorre veloce la partita aperta; si regola solo durante una partita.",
        "How fast the open game runs; it is set only during a game.",
    ),
    (
        "Pioggia e neve sulla mappa, con il loro effetto sulle unità.",
        "Rain and snow on the map, with their effect on the units.",
    ),
    (
        "Il passare del giorno e della notte, che cambia luce e visibilità.",
        "The passing of day and night, which changes light and sight.",
    ),
    (
        "Il pannello dei trucchi di questa partita (tasto F7).",
        "The cheat panel of this game (F7 key).",
    ),
    (
        "Quanto velocemente si sposta la mappa con frecce e bordi dello schermo.",
        "How fast the map moves with the arrows and the screen's edges.",
    ),
    (
        "La mappa si sposta quando porti il puntatore ai bordi dello schermo.",
        "The map moves when you bring the pointer to the screen's edges.",
    ),
    (
        "Il nome mostrato sulle tue unità.",
        "The name shown over your units.",
    ),
    (
        "Apri l'elenco e scegli la lingua di testi e voci.",
        "Open the list and choose the language of texts and voices.",
    ),
    (
        "Le missioni delle campagne già raggiunte restano da scegliere; l'originale mostra solo la prima.",
        "The campaign missions already reached stay to choose from; the original lists only the first.",
    ),
    (
        "Il gioco occupa tutto lo schermo, senza bordi; altrimenti è in una finestra (anche Alt+Invio).",
        "The game fills the whole screen, borderless; otherwise it is in a window (Alt+Enter too).",
    ),
    (
        "Trascina i bordi della finestra per cambiarne le dimensioni, che restano per la prossima volta; qui ne scegli una pronta.",
        "Drag the window's edges to resize it, and the size stays for next time; here you pick a ready-made one.",
    ),
    (
        "I fotogrammi seguono la frequenza dello schermo, senza strappi nell'immagine; il limite dei fotogrammi allora non serve e si spegne.",
        "The frames follow the display's refresh rate, without tearing; the frame limit is then of no use and switches off.",
    ),
    (
        "I fotogrammi al secondo massimi: trascina la barra o scrivi il numero; in fondo alla barra nessun limite. Spento con la sincronizzazione verticale.",
        "The most frames per second: drag the bar or type the number; at the bar's end there is no limit. Off with vertical sync.",
    ),
    (
        "Pixel nitidi: l'interfaccia ingrandita di un numero intero di volte. Adatta: riempie lo schermo.",
        "Sharp pixels: the interface enlarged a whole number of times. Fit: it fills the screen.",
    ),
    (
        "Da che parte dello schermo sta il pannello dei comandi durante la partita.",
        "Which side of the screen holds the command panel during a game.",
    ),
    (
        "Il puntatore disegnato dal gioco o quello di Windows, con lo stesso disegno.",
        "The pointer drawn by the game or Windows' own, with the same picture.",
    ),
    (
        "Come si leggono messaggi e consigli sulla mappa: con l'ombra originale, un contorno nero o un fondo scuro.",
        "How messages and hints read on the map: with the original shadow, a black outline or a dark band.",
    ),
    (
        "Una griglia con le caselle libere e occupate mentre piazzi un edificio, e quanto si vede.",
        "A grid of the free and blocked cells while you place a building, and how strong it is.",
    ),
    (
        "Sfumature lisce di notte, nelle ombre e nella nebbia, senza i 5 bit per colore dell'originale.",
        "Smooth shades at night, in shadows and fog, without the original's 5 bits per color.",
    ),
    (
        "Pixel originali, regolari a ogni zoom o levigati, con i bordi smussati.",
        "Original pixels, even at every zoom, or smoothed with softened edges.",
    ),
    (
        "Righe e curvatura di un vecchio monitor.",
        "Scan lines and curve of an old monitor.",
    ),
    (
        "Testi dell'interfaccia nitidi alla risoluzione dello schermo.",
        "Sharp interface texts at the screen's resolution.",
    ),
    (
        "Di notte fuochi, edifici e magie fanno luce intorno.",
        "At night fires, buildings and magic light up their surroundings.",
    ),
    (
        "Un alone luminoso intorno a fuochi, magie e fulmini.",
        "A bright glow around fire, magic and lightning.",
    ),
    (
        "Bordi sfumati della nebbia invece dei riquadri.",
        "Soft fog edges instead of squares.",
    ),
    (
        "Alberi che ondeggiano al vento e acqua che luccica.",
        "Trees swaying in the wind and glinting water.",
    ),
    (
        "La rotella ingrandisce scorrendo, con più passi.",
        "The wheel zooms smoothly, in more steps.",
    ),
    (
        "Fai clic e premi il nuovo tasto del comando; Esc annulla.",
        "Click, then press the command's new key; Esc cancels.",
    ),
    (
        "Rimette le impostazioni di questa scheda come all'inizio; nome, lingua e dimensioni della finestra restano.",
        "Puts the settings of this tab back as they first were; name, language and window size stay.",
    ),
    (
        "Chiude rimettendo tutto com'era all'apertura delle opzioni (anche Esc).",
        "Closes putting everything back as it was when the settings opened (Esc too).",
    ),
    // Other controls this version adds.
    ("Scorrimento", "Scroll"),
    ("Resa dell'IA", "AI surrender"),
    (
        "RESA DELL'IA: con la BANDIERA bianca i regni del computer si arrendono quando perdono, con le SPADE combattono fino alla fine",
        "AI SURRENDER: with the white FLAG the computer's kingdoms surrender when they are losing, with the SWORDS they fight to the end",
    ),
    // The commands of the keys page.
    ("Scorri a sinistra", "Scroll left"),
    ("Scorri a destra", "Scroll right"),
    ("Scorri in alto", "Scroll up"),
    ("Scorri in basso", "Scroll down"),
    ("Centra la selezione", "Center the selection"),
    ("Unità successiva del gruppo", "Next unit of the group"),
    ("Ultimo evento", "Latest event"),
    ("Evento precedente", "Previous event"),
    ("Evento successivo", "Next event"),
    ("Destinazione o raduno", "Destination or rally point"),
    ("Barre della salute", "Health bars"),
    ("Alberi trasparenti", "Transparent trees"),
    ("Elimina la selezione", "Delete the selection"),
    ("Aumenta la velocità", "Increase the speed"),
    ("Riduci la velocità", "Decrease the speed"),
    ("Salvataggio rapido", "Quick save"),
    ("Caricamento rapido", "Quick load"),
    ("Punteggio e tempo", "Score and time"),
    (
        "Pagina precedente dell'inventario",
        "Previous inventory page",
    ),
    ("Pagina seguente dell'inventario", "Next inventory page"),
    ("Aiuto", "Help"),
    ("Diplomazia", "Diplomacy"),
    ("Obiettivi", "Objectives"),
    ("Opzioni", "Options"),
    ("Menu della partita", "Game menu"),
    ("Conversazione", "Chat"),
    ("Tributi", "Tribute"),
    ("Pannello dei trucchi", "Cheat panel"),
    // Key names.
    ("Spazio", "Space"),
    ("Freccia sinistra", "Left arrow"),
    ("Freccia destra", "Right arrow"),
    ("Freccia su", "Up arrow"),
    ("Freccia giù", "Down arrow"),
    ("Invio", "Enter"),
    ("Canc", "Delete"),
    ("Ins", "Insert"),
    ("Fine", "End"),
    ("Pag su", "Page Up"),
    ("Pag giù", "Page Down"),
    ("Tasto Indietro", "Backspace"),
    ("+ (tastierino)", "+ (keypad)"),
    ("- (tastierino)", "- (keypad)"),
    ("* (tastierino)", "* (keypad)"),
    ("/ (tastierino)", "/ (keypad)"),
    ("Invio (tastierino)", "Enter (keypad)"),
    ("{} (tastierino)", "{} (keypad)"),
    // The menus' notices.
    ("Inizializzazione dell'audio…", "Starting the audio…"),
    ("Audio non disponibile", "Audio not available"),
    (
        "Impostazioni non leggibili ({}): ripristinati i valori predefiniti.",
        "Settings could not be read ({}): the defaults are restored.",
    ),
    (
        "La modalità multigiocatore non è disponibile in questa versione.",
        "Multiplayer is not available in this version.",
    ),
    ("Eroe", "Hero"),
    ("Nessuna missione selezionata", "No mission selected"),
    ("(missione non installata)", "(mission not installed)"),
    (
        "La missione del salvataggio non è presente",
        "The saved game's mission is not installed",
    ),
    ("Missione non trovata", "Mission not found"),
    ("Mappa casuale {}", "Random map {}"),
    ("{} · {}×{} · seme {}", "{} · {}×{} · seed {}"),
    ("Caricamento di «{}»…", "Loading “{}”…"),
    ("Ripristino di «{}»…", "Restoring “{}”…"),
    (
        "Un salvataggio è già in corso",
        "A save is already in progress",
    ),
    (
        "Attendi il completamento del salvataggio",
        "Wait for the save to finish",
    ),
    ("Nessuna mappa aperta", "No map is open"),
    ("Simulazione non inizializzata", "The game has not started"),
    ("Salvataggio interrotto", "Save interrupted"),
    ("Salvataggio non riuscito: {}", "Save failed: {}"),
    (
        "Impossibile riprodurre intro: {}",
        "Cannot play the intro: {}",
    ),
    // The random maps' types, named as the American release names them.
    ("Casuale", "Random"),
    ("Arcipelaghi", "Archipelagos"),
    ("Isole", "Island"),
    ("Continente", "Continental"),
    ("Laghi", "Lakes"),
    ("Terra", "Dry"),
    // The interface of the map.
    (
        "Annulla e restituisci il costo",
        "Cancel and refund the cost",
    ),
    ("Indietro", "Back"),
    (
        "Trucchi\nApre la finestra dei trucchi di questa partita",
        "Cheats\nOpens the cheat window of this game",
    ),
    ("Trucchi", "Cheats"),
    (
        "Apre la finestra dei trucchi di questa partita",
        "Opens the cheat window of this game",
    ),
    (
        "Usato un trucco, la partita resta segnata come partita con trucchi",
        "A cheat was used: the game stays marked as a game with cheats",
    ),
    ("Chiude la finestra dei trucchi", "Closes the cheat window"),
    // The cheat panel.
    ("TRUCCHI", "CHEATS"),
    ("CHIUDI", "CLOSE"),
    ("Partita", "Game"),
    (
        "Risorse, popolazione, costruzione, nebbia, mappa, ora del giorno, tecnologie e missione",
        "Resources, population, building, fog, map, time of day, technologies and mission",
    ),
    ("Oggetti", "Objects"),
    (
        "Trucchi da applicare a un'unità o a un edificio cliccandolo sulla mappa",
        "Cheats to apply to a unit or a building by clicking it on the map",
    ),
    ("Unità", "Units"),
    (
        "Crea un'unità di qualunque regno nel punto della mappa che scegli",
        "Creates a unit of any kingdom at the point of the map you choose",
    ),
    ("Edifici", "Buildings"),
    (
        "Crea un edificio già costruito di qualunque regno nel punto della mappa che scegli",
        "Creates a finished building of any kingdom at the point of the map you choose",
    ),
    (
        "Cambia a piacere i rapporti fra due regni",
        "Changes the relations between two kingdoms at will",
    ),
    ("Nessun regno", "No kingdom"),
    ("Regno {}", "Kingdom {}"),
    ("Regno", "Kingdom"),
    ("Europea", "European"),
    ("Araba", "Arabian"),
    ("Asiatica", "Asian"),
    ("europea", "European"),
    ("araba", "Arabian"),
    ("asiatica", "Asian"),
    ("Alleato", "Ally"),
    (
        "{a} considera {b} un alleato: non lo attacca",
        "{a} regards {b} as an ally: it does not attack it",
    ),
    ("Neutrale", "Neutral"),
    (
        "{a} considera {b} neutrale: non lo attacca",
        "{a} regards {b} as neutral: it does not attack it",
    ),
    ("Nemico", "Enemy"),
    (
        "{a} considera {b} un nemico e lo attacca",
        "{a} regards {b} as an enemy and attacks it",
    ),
    ("Condivide la vista", "Shares vision"),
    (
        "{b} vede ciò che vedono le unità di {a}",
        "{b} sees what the units of {a} see",
    ),
    ("Condivide le risorse", "Shares resources"),
    (
        "{b} può spendere le risorse di {a}",
        "{b} can spend the resources of {a}",
    ),
    ("Condivide le unità", "Shares units"),
    (
        "{b} può comandare le unità di {a}",
        "{b} can command the units of {a}",
    ),
    ("Vittoria condivisa", "Shared victory"),
    ("{a} vince insieme a {b}", "{a} wins together with {b}"),
    ("Vuole la guerra", "Wants war"),
    (
        "{a} vuole la guerra con {b}: basta che uno dei due la voglia e diventano nemici",
        "{a} wants war with {b}: if either of them wants it, they become enemies",
    ),
    ("Vuole la pace", "Wants peace"),
    (
        "{a} vuole la pace con {b}: quando la vogliono tutti e due diventano neutrali",
        "{a} wants peace with {b}: when both want it, they become neutral",
    ),
    ("Vuole l'alleanza", "Wants alliance"),
    (
        "{a} vuole l'alleanza con {b}: quando la vogliono tutti e due diventano alleati",
        "{a} wants an alliance with {b}: when both want it, they become allies",
    ),
    ("Costo", "Cost"),
    (
        " (In questa partita le risorse sono illimitate.)",
        " (Resources are unlimited in this game.)",
    ),
    ("Risorse", "Resources"),
    (
        "Aggiunge oro, pietra, cibo e legna al tuo regno",
        "Adds gold, stone, food and wood to your kingdom",
    ),
    (
        "Aggiunge 10 000 di oro, pietra, cibo e legna al tuo regno.",
        "Adds 10,000 gold, stone, food and wood to your kingdom.",
    ),
    (
        "Aggiunge 50 000 di oro, pietra, cibo e legna al tuo regno.",
        "Adds 50,000 gold, stone, food and wood to your kingdom.",
    ),
    ("Popolazione massima", "Population limit"),
    (
        "Il limite della popolazione del tuo regno",
        "Your kingdom's population limit",
    ),
    ("Porta a 1000", "Raise to 1000"),
    ("Già a 1000", "Already 1000"),
    (
        "Porta a 1000 il limite della popolazione del tuo regno, senza bisogno di case",
        "Raises your kingdom's population limit to 1000, with no need for houses",
    ),
    ("Costruzione immediata", "Instant building"),
    (
        "Cantieri, unità e ricerche si completano subito",
        "Construction sites, units and research are finished at once",
    ),
    (
        "Quando è attiva, ogni cantiere si completa al primo colpo di un contadino e le unità e le ricerche escono subito, per tutti i regni. Clicca per attivarla o spegnerla",
        "When on, every construction site is finished by a peasant's first blow and units and research come out at once, for every kingdom. Click to switch it on or off",
    ),
    ("Nessun requisito", "No requirements"),
    (
        "Costruire, creare e ricercare senza i requisiti",
        "Build, create and research without the requirements",
    ),
    (
        "Quando è attivo, si possono costruire edifici, creare unità e fare ricerche senza gli edifici e le tecnologie che servirebbero, per tutti i regni. Clicca per attivarlo o spegnerlo",
        "When on, buildings can be built, units created and research done without the buildings and technologies they would need, for every kingdom. Click to switch it on or off",
    ),
    ("Nebbia di guerra", "Fog of war"),
    (
        "La nebbia che nasconde ciò che le tue unità non vedono",
        "The fog that hides what your units cannot see",
    ),
    ("Togli", "Remove"),
    ("Già tolta", "Already removed"),
    (
        "Toglie la nebbia di guerra per il resto della partita: vedi tutto ciò che si muove sulla mappa",
        "Removes the fog of war for the rest of the game: you see everything that moves on the map",
    ),
    ("Mappa", "Map"),
    (
        "Le zone della mappa non ancora esplorate",
        "The areas of the map not yet explored",
    ),
    ("Rivela tutta", "Reveal all"),
    ("Già esplorata", "Already explored"),
    (
        "Rende esplorata tutta la mappa: vedi il terreno e gli edifici, ma le unità solo dove arriva la vista delle tue",
        "Explores the whole map: you see the ground and the buildings, but units only where your own units can see",
    ),
    ("Ora del giorno", "Time of day"),
    (
        "L'ora dell'orologio della partita, che decide la luce del giorno e della notte",
        "The hour of the game's clock, which sets the light of day and night",
    ),
    (
        "Un'ora indietro (con Maiusc dieci)",
        "One hour back (ten with Shift)",
    ),
    (
        "Un'ora avanti (con Maiusc dieci)",
        "One hour on (ten with Shift)",
    ),
    (
        "Cambia l'ora del giorno con le frecce",
        "Change the time of day with the arrows",
    ),
    ("Tecnologia", "Technology"),
    (
        "Una ricerca del tuo regno, completata senza aspettare né pagare",
        "A research of your kingdom, completed without waiting or paying",
    ),
    ("nessuna da scoprire", "none left to discover"),
    ("La tecnologia precedente", "The previous technology"),
    ("La tecnologia seguente", "The next technology"),
    ("Scopri", "Discover"),
    (
        "Completa subito per il tuo regno: {}",
        "Completes it at once for your kingdom: {}",
    ),
    ("Animali e mostri", "Animals and monsters"),
    (
        "Le unità della Natura: animali selvatici e mostri",
        "Nature's units: wild animals and monsters",
    ),
    ("Togli dalla mappa", "Remove from the map"),
    ("Nessuno sulla mappa", "None on the map"),
    (
        "Fa sparire dalla mappa tutte le unità della Natura: animali selvatici e mostri",
        "Removes all of Nature's units from the map: wild animals and monsters",
    ),
    ("Missione", "Mission"),
    (
        "La missione di questa campagna",
        "The mission of this campaign",
    ),
    ("Passa alla successiva", "Skip to the next"),
    ("Nessuna successiva", "No next mission"),
    ("Solo nelle campagne", "Campaigns only"),
    (
        "Lascia questa missione e apre subito la mappa successiva della campagna",
        "Leaves this mission and opens the campaign's next map at once",
    ),
    (
        "Scegli un trucco: la finestra si chiude e clicchi sulla mappa l'unità o l'edificio a cui applicarlo. Tieni premuto Maiusc per cliccarne altri, tasto destro per finire.",
        "Choose a cheat: the window closes and you click on the map the unit or building to apply it to. Hold Shift to click more, right-click to finish.",
    ),
    ("Invulnerabile", "Invulnerable"),
    (
        "Non subisce più danni. Un altro clic lo rende di nuovo vulnerabile.",
        "It takes no more damage. Another click makes it vulnerable again.",
    ),
    (
        "Rende invulnerabile l'oggetto che clicchi: non subisce più danni. Cliccarlo di nuovo lo rende di nuovo vulnerabile",
        "Makes the object you click invulnerable: it takes no more damage. Clicking it again makes it vulnerable again",
    ),
    ("Prendi il comando", "Take command"),
    (
        "Gli dai ordini anche se è di un altro regno. Un altro clic lo restituisce.",
        "You give it orders even if it belongs to another kingdom. Another click gives it back.",
    ),
    (
        "Puoi dare ordini all'oggetto che clicchi anche se è di un altro regno. Cliccarlo di nuovo lo restituisce",
        "You can give orders to the object you click even if it belongs to another kingdom. Clicking it again gives it back",
    ),
    ("Guarda con i suoi occhi", "See through its eyes"),
    (
        "Vedi ciò che vede, anche se è di un altro regno. Un altro clic smette.",
        "You see what it sees, even if it belongs to another kingdom. Another click stops.",
    ),
    (
        "Vedi ciò che vede l'oggetto che clicchi, anche se è di un altro regno. Cliccarlo di nuovo smette",
        "You see what the object you click sees, even if it belongs to another kingdom. Clicking it again stops",
    ),
    ("Dai al regno", "Give to kingdom"),
    (
        "L'oggetto che clicchi passa al regno scelto: {}",
        "The object you click goes to the chosen kingdom: {}",
    ),
    ("Il regno precedente", "The previous kingdom"),
    ("Il regno seguente", "The next kingdom"),
    (
        "Il regno a cui passa l'oggetto che clicchi",
        "The kingdom the object you click goes to",
    ),
    (
        "Crea le unità e gli edifici della civiltà {}",
        "Creates the units and buildings of the {} civilization",
    ),
    (
        "Il regno a cui va quello che crei",
        "The kingdom what you create goes to",
    ),
    (
        "Il regno di cui cambi i rapporti",
        "The kingdom whose relations you change",
    ),
    (
        "Il regno verso cui cambiano",
        "The kingdom they change toward",
    ),
    (
        "Come {} considera {}: clicca una casella per accenderla o spegnerla.",
        "How {} regards {}: click a box to switch it on or off.",
    ),
    (
        "Scegli due regni diversi.",
        "Choose two different kingdoms.",
    ),
    ("{}. Ora: {}", "{}. Now: {}"),
    ("sì", "yes"),
    ("un'unità", "a unit"),
    (
        "un edificio, che arriva già costruito",
        "a building, which arrives already built",
    ),
    (
        "Clicca {}, poi il punto della mappa dove metterlo. Tieni premuto Maiusc per metterne altri, tasto destro per finire.",
        "Click {}, then the point of the map where to put it. Hold Shift to put more, right-click to finish.",
    ),
    ("Partita con trucchi: {}", "Game with cheats: {}"),
    (
        "Maiusc per continuare, tasto destro per finire.",
        "Shift to go on, right-click to finish.",
    ),
    (
        "Clicca sulla mappa dove mettere: {}. {}",
        "Click on the map where to put: {}. {}",
    ),
    (
        "Clicca l'oggetto da rendere invulnerabile (o di nuovo vulnerabile).",
        "Click the object to make invulnerable (or vulnerable again).",
    ),
    (
        "Clicca l'oggetto di cui prendere il comando (o da restituire).",
        "Click the object to take command of (or to give back).",
    ),
    (
        "Clicca l'oggetto con cui guardare (o con cui smettere).",
        "Click the object to see through (or to stop).",
    ),
    (
        "Clicca l'oggetto da dare al regno scelto.",
        "Click the object to give to the chosen kingdom.",
    ),
    // Saved games.
    (
        "Versione della sessione non supportata",
        "Session version not supported",
    ),
    ("Archivio della mappa non valido", "Invalid map archive"),
    (
        "Dimensioni o stato della sessione non validi",
        "Invalid session size or state",
    ),
    (
        "Riferimenti dell'inventario non validi",
        "Invalid inventory references",
    ),
    (
        "Contenuto dell'inventario non valido",
        "Invalid inventory contents",
    ),
    (
        "Collegamento dell'inventario non valido",
        "Invalid inventory link",
    ),
    ("Ciclo nell'inventario", "Loop in an inventory"),
    (
        "Riferimenti della mappa o dell'interfaccia non validi",
        "Invalid map or interface references",
    ),
    ("Ritratto salvato non valido", "Invalid saved portrait"),
    ("Salvataggio non valido", "Invalid saved game"),
    ("Salvataggio non riconosciuto", "Saved game not recognized"),
    (
        "Versione del salvataggio non supportata",
        "Saved game version not supported",
    ),
    (
        "Salvataggio troppo grande o danneggiato",
        "Saved game too large or damaged",
    ),
    (
        "Metadati del salvataggio non validi",
        "Invalid saved game details",
    ),
    ("Salvataggio troncato", "Saved game cut short"),
    (
        "Lunghezza del salvataggio non valida",
        "Invalid saved game length",
    ),
    (
        "Controllo d'integrità del salvataggio fallito",
        "Saved game failed its integrity check",
    ),
    (
        "Sessione compressa danneggiata",
        "Compressed session damaged",
    ),
    (
        "Salvataggio di una versione precedente di ReTzar: non si può più riprendere",
        "Saved by an earlier version of ReTzar: it can no longer be resumed",
    ),
    ("Salvataggio troppo grande", "Saved game too large"),
    (
        "Percorso del salvataggio non valido",
        "Invalid saved game path",
    ),
    (
        "Dato della mappa non riconosciuto",
        "Map data not recognized",
    ),
    ("Dato della mappa troppo grande", "Map data too large"),
    ("Dato della mappa danneggiato", "Map data damaged"),
    (
        "Archivio della mappa troppo grande",
        "Map archive too large",
    ),
    (
        "Salvataggio con dati della mappa non risolti",
        "Saved game with unresolved map data",
    ),
    // Starting a map, and the errors that stop the program.
    (
        "Regno {}: nessuna posizione iniziale nella mappa",
        "Kingdom {}: no starting position on the map",
    ),
    (
        "Regno {}: spazio insufficiente per il castello",
        "Kingdom {}: not enough room for the castle",
    ),
    (
        "Regno {}: spazio insufficiente per i contadini",
        "Kingdom {}: not enough room for the peasants",
    ),
    (
        "Identificatori degli oggetti esauriti",
        "No object identifiers left",
    ),
    (
        "Nessun archivio del gioco trovato",
        "No archive of the game found",
    ),
    (
        "Installazione di Tzar non trovata vicino a {}. Usa --source CARTELLA.",
        "Tzar installation not found near {}. Use --source FOLDER.",
    ),
    (
        "ReTzar non può continuare:\n\n{}",
        "ReTzar cannot continue:\n\n{}",
    ),
    (
        "ReTzar si è chiuso per un errore interno.\n\n{}\n\nDettagli in crash.log.",
        "ReTzar closed because of an internal error.\n\n{}\n\nDetails in crash.log.",
    ),
    (
        "Recuperata la precedente copia valida del salvataggio.",
        "The previous valid copy of the save was recovered.",
    ),
    (
        "Salvataggio precedente: ripristinata la vista iniziale, senza stato della simulazione.",
        "Older save: the starting view was restored, without the state of the simulation.",
    ),
    (
        "Nessuna scheda video Vulkan trovata. ReTzar disegna solo con Vulkan: aggiorna i driver della scheda video.",
        "No Vulkan graphics card found. ReTzar draws only with Vulkan: update the graphics card's drivers.",
    ),
    // The portable copy's original copy (`original.rs`).
    (
        "Scegli la cartella in cui è installato Tzar",
        "Choose the folder where Tzar is installed",
    ),
    (
        "Per giocare con ReTzar serve una copia originale di Tzar (dal CD, da GOG o da Steam): alla prossima partenza scegli la cartella in cui è installata.",
        "To play ReTzar you need an original copy of Tzar (from the CD, GOG or Steam): at the next start, choose the folder where it is installed.",
    ),
    (
        "In {} non c'è una copia originale di Tzar.\n\nScegli la cartella in cui è installato il gioco.",
        "There is no original copy of Tzar in {}.\n\nChoose the folder where the game is installed.",
    ),
    // The first start of the portable copy (`first_start.rs`).
    (
        "Benvenuto in ReTzar! Il gioco usa la tua copia di Tzar (GOG, Steam o CD): scegli la cartella in cui è installato e ReTzar ne ricaverà i suoi dati. Serve solo la prima volta.",
        "Welcome to ReTzar! The game uses your own copy of Tzar (GOG, Steam or CD): choose the folder where it is installed and ReTzar will make its data from it. This is needed only the first time.",
    ),
    (
        "ReTzar non può scrivere nella sua cartella ({}): spostalo in una cartella tua, per esempio sul Desktop o in Documenti.",
        "ReTzar cannot write in its folder ({}): move it to a folder of yours, for example on the Desktop or in Documents.",
    ),
    (
        "ReTzar sta creando i dati del gioco dalla tua copia di Tzar…",
        "ReTzar is making the game's data from your copy of Tzar…",
    ),
    ("Controllo dei dati…", "Checking the data…"),
    (
        "Non è stato possibile creare i dati del gioco: {}",
        "The game's data could not be made: {}",
    ),
];

/// The catalog of ReTzar's own texts in a language translated by this
/// version: pairs of the Italian, exactly as written in the code, and its
/// translation, in the form of the missions' catalogs.
pub fn catalog(language: super::Language) -> Option<&'static str> {
    use super::Language::*;
    Some(match language {
        French => include_str!("../../languages/fr/own.txt"),
        German => include_str!("../../languages/de/own.txt"),
        Spanish => include_str!("../../languages/es/own.txt"),
        Polish => include_str!("../../languages/pl/own.txt"),
        Portuguese => include_str!("../../languages/pt/own.txt"),
        Italian | English => return None,
    })
}
