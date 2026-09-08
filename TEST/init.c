/*
 * init.c — semplice processo "init" (PID 1) per un container.
 *
 * Comportamento:
 *   1. Legge la directory SCRIPTS_DIR (di default "/TEST/scripts").
 *   2. Seleziona solo i file regolari con permesso di esecuzione.
 *   3. Li ordina lessicograficamente (strcmp, byte-per-byte, come "ls" in locale C).
 *   4. Li esegue UNO ALLA VOLTA, in sequenza, aspettando che ognuno termini
 *      prima di lanciare il successivo.
 *   5. Per ogni script stampa nome file e timestamp di avvio, e al termine
 *      stampa tempo trascorso (wall clock) e risorse usate (CPU utente/
 *      sistema, memoria massima residente) prelevate con wait4()/rusage.
 *      Tutti i messaggi di init vengono scritti su STDOUT (non stderr) e
 *      con buffering disattivato: così condividono lo stesso stream/fd
 *      ereditato dagli script (che di norma scrivono anch'essi su stdout),
 *      e il kernel garantisce che le write() sullo stesso fd/pipe arrivino
 *      nell'ordine reale in cui sono state eseguite. Questo elimina quasi
 *      del tutto il problema di messaggi di init che compaiono prima
 *      dell'output (bufferizzato) di uno script. Come ulteriore rete di
 *      sicurezza per i casi residui (es. script che scrivono su stderr, o
 *      un multiplexer esterno con latenza propria), dopo ogni script viene
 *      atteso un piccolo ritardo fisso (POST_SCRIPT_DELAY_NS) prima di
 *      stampare il riepilogo.
 *   6. Essendo PID 1, fa comunque "reaping" di eventuali processi orfani
 *      (adottati dal kernel) dopo ogni script, per evitare zombie residui.
 *   7. Al termine di TUTTI gli script il processo esce (niente loop finale):
 *      il container termina quando la sequenza è completata.
 *   8. Argomenti da riga di comando nel formato "env:CHIAVE=VALORE" vengono
 *      aggiunti (o sovrascritti se già presenti) all'environment con cui
 *      vengono lanciati gli script, senza toccare l'ambiente di init stesso.
 *
 * Compilazione:
 *   gcc -O2 -Wall -o init init.c
 *
 * Uso tipico in un container:
 *   ENTRYPOINT ["/init"]
 *
 * Argomenti supportati (in qualunque ordine):
 *   /init [directory] [env:CHIAVE=VALORE ...]
 *
 * Esempio:
 *   /init /TEST/scripts env:AMBIENTE=produzione env:DEBUG=0
 */

#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define DEFAULT_SCRIPTS_DIR "/TEST/scripts"
#define ENV_PREFIX "env:"

/* Ritardo fisso applicato dopo ogni script, come rete di sicurezza per
 * lasciar defluire eventuale output non ancora consegnato al collector
 * di log esterno (es. il supervisore del container runtime). Non risolve
 * problemi di buffering interni allo script stesso, ma copre i casi in
 * cui la determinazione esatta non è possibile dall'esterno del processo. */
#define POST_SCRIPT_DELAY_NS (50L * 1000 * 1000) /* 50 ms */

extern char **environ;

static int cmp_str(const void *a, const void *b) {
    const char *sa = *(const char *const *)a;
    const char *sb = *(const char *const *)b;
    return strcmp(sa, sb);
}

/* Reaping non bloccante di eventuali processi orfani già terminati,
 * senza interferire con il wait bloccante sul figlio "atteso". */
static void reap_orphans(void) {
    int status;
    while (waitpid(-1, &status, WNOHANG) > 0) {
        /* processo orfano reaped, nulla da fare */
    }
}

/* Timestamp leggibile "YYYY-MM-DD HH:MM:SS" dell'istante corrente. */
static void current_timestamp(char *buf, size_t bufsz) {
    time_t now = time(NULL);
    struct tm tm_now;
    localtime_r(&now, &tm_now);
    strftime(buf, bufsz, "%Y-%m-%d %H:%M:%S", &tm_now);
}

/* Differenza in secondi (con decimali) tra due struct timespec. */
static double timespec_diff(const struct timespec *end, const struct timespec *start) {
    return (end->tv_sec - start->tv_sec)
         + (end->tv_nsec - start->tv_nsec) / 1e9;
}

/* Piccola pausa fissa, best-effort, per dare tempo a un eventuale output
 * già scritto ma non ancora "consegnato" dal collector di log esterno. */
static void small_delay(void) {
    struct timespec ts = { .tv_sec = 0, .tv_nsec = POST_SCRIPT_DELAY_NS };
    nanosleep(&ts, NULL);
}

/* Ritorna 1 se la entry di environ "KEY=..." ha la stessa chiave di key
 * (stringa senza '='), altrimenti 0. */
static int env_entry_has_key(const char *entry, const char *key, size_t keylen) {
    return strncmp(entry, key, keylen) == 0 && entry[keylen] == '=';
}

/*
 * Costruisce l'array envp da usare per gli script:
 *   - parte dall'environment corrente di init (environ)
 *   - esclude le chiavi che vengono sovrascritte dagli "extra"
 *   - appende in coda tutte le variabili "extra" (formato "CHIAVE=VALORE")
 * Il risultato va liberato con free() (il singolo array, non le stringhe:
 * puntano a memoria già esistente in environ o in extra).
 */
static char **build_envp(char **extra, size_t extra_n) {
    size_t base_n = 0;
    while (environ[base_n] != NULL)
        base_n++;

    char **envp = malloc((base_n + extra_n + 1) * sizeof(char *));
    if (!envp) {
        fprintf(stdout, "init: memoria esaurita nella costruzione dell'environment\n");
        exit(1);
    }

    size_t idx = 0;
    for (size_t i = 0; i < base_n; i++) {
        int overridden = 0;
        for (size_t j = 0; j < extra_n; j++) {
            char *eq = strchr(extra[j], '=');
            size_t keylen = eq ? (size_t)(eq - extra[j]) : strlen(extra[j]);
            if (env_entry_has_key(environ[i], extra[j], keylen)) {
                overridden = 1;
                break;
            }
        }
        if (!overridden)
            envp[idx++] = environ[i];
    }
    for (size_t j = 0; j < extra_n; j++)
        envp[idx++] = extra[j];
    envp[idx] = NULL;

    return envp;
}

int main(int argc, char *argv[]) {
    /* Buffering disattivato: ogni fprintf(stdout, ...) di init diventa
     * immediatamente una write() sullo stesso fd ereditato dagli script,
     * preservando l'ordine reale degli eventi. */
    setvbuf(stdout, NULL, _IONBF, 0);

    const char *scripts_dir = DEFAULT_SCRIPTS_DIR;

    char **extra_env = NULL;
    size_t extra_n = 0, extra_cap = 0;

    for (int i = 1; i < argc; i++) {
        if (strncmp(argv[i], ENV_PREFIX, strlen(ENV_PREFIX)) == 0) {
            char *rest = argv[i] + strlen(ENV_PREFIX); /* "CHIAVE=VALORE" */
            char *eq = strchr(rest, '=');
            if (!eq || eq == rest) {
                fprintf(stdout,
                        "init: argomento env non valido, ignorato: %s\n",
                        argv[i]);
                continue;
            }
            if (extra_n == extra_cap) {
                extra_cap = extra_cap ? extra_cap * 2 : 8;
                char **tmp = realloc(extra_env, extra_cap * sizeof(char *));
                if (!tmp) {
                    fprintf(stdout, "init: memoria esaurita\n");
                    exit(1);
                }
                extra_env = tmp;
            }
            extra_env[extra_n++] = strdup(rest); /* "CHIAVE=VALORE" */
        } else {
            scripts_dir = argv[i];
        }
    }

    if (extra_n > 0) {
        fprintf(stdout, "init: variabili d'ambiente aggiuntive per gli script:\n");
        for (size_t j = 0; j < extra_n; j++)
            fprintf(stdout, "init:   %s\n", extra_env[j]);
    }

    char **envp = build_envp(extra_env, extra_n);

    if (getpid() != 1) {
        fprintf(stdout, "init: attenzione, non sto girando come PID 1\n");
    }

    DIR *d = opendir(scripts_dir);
    if (!d) {
        fprintf(stdout, "init: impossibile aprire %s: %s\n",
                scripts_dir, strerror(errno));
    } else {
        char **names = NULL;
        size_t n = 0, cap = 0;
        struct dirent *ent;

        while ((ent = readdir(d)) != NULL) {
            if (strcmp(ent->d_name, ".") == 0 || strcmp(ent->d_name, "..") == 0)
                continue;

            char full[4096];
            snprintf(full, sizeof(full), "%s/%s", scripts_dir, ent->d_name);

            struct stat st;
            if (stat(full, &st) != 0 || !S_ISREG(st.st_mode))
                continue;
            if (access(full, X_OK) != 0)
                continue;

            if (n == cap) {
                cap = cap ? cap * 2 : 16;
                char **tmp = realloc(names, cap * sizeof(char *));
                if (!tmp) {
                    fprintf(stdout, "init: memoria esaurita\n");
                    break;
                }
                names = tmp;
            }
            names[n++] = strdup(ent->d_name);
        }
        closedir(d);

        qsort(names, n, sizeof(char *), cmp_str);

        for (size_t i = 0; i < n; i++) {
            char full[4096];
            snprintf(full, sizeof(full), "%s/%s", scripts_dir, names[i]);

            char ts[32];
            current_timestamp(ts, sizeof(ts));
            fprintf(stdout, "init: [%s] eseguo %s\n", ts, full);

            struct timespec t_start, t_end;
            clock_gettime(CLOCK_MONOTONIC, &t_start);

            pid_t pid = fork();
            if (pid < 0) {
                fprintf(stdout, "init: fork fallita per %s: %s\n",
                        full, strerror(errno));
                free(names[i]);
                continue;
            }

            if (pid == 0) {
                char *child_argv[] = { full, NULL };
                execve(full, child_argv, envp);
                fprintf(stdout, "init: execve fallita per %s: %s\n",
                        full, strerror(errno));
                _exit(127);
            }

            int status;
            struct rusage usage;
            /* wait4 raccoglie anche le risorse usate da QUESTO figlio,
             * dato che eseguiamo un solo script alla volta in sequenza. */
            wait4(pid, &status, 0, &usage);
            clock_gettime(CLOCK_MONOTONIC, &t_end);

            /* Rete di sicurezza: piccola pausa fissa prima di stampare il
             * nostro riepilogo, per lasciar defluire eventuale output
             * dello script non ancora consegnato al collector di log. */
            small_delay();

            double elapsed   = timespec_diff(&t_end, &t_start);
            double cpu_user  = usage.ru_utime.tv_sec + usage.ru_utime.tv_usec / 1e6;
            double cpu_sys   = usage.ru_stime.tv_sec + usage.ru_stime.tv_usec / 1e6;
            long   maxrss_kb = usage.ru_maxrss; /* già in KB su Linux */

            if (WIFEXITED(status)) {
                fprintf(stdout, "init: %s terminato con codice %d\n",
                        full, WEXITSTATUS(status));
            } else if (WIFSIGNALED(status)) {
                fprintf(stdout, "init: %s terminato dal segnale %d\n",
                        full, WTERMSIG(status));
            }
            fprintf(stdout,
                    "init:   tempo trascorso=%.3fs  cpu_utente=%.3fs  "
                    "cpu_sistema=%.3fs  mem_max_residente=%ldKB\n",
                    elapsed, cpu_user, cpu_sys, maxrss_kb);

            reap_orphans();
            free(names[i]);
        }
        free(names);
    }

    free(envp);
    small_delay();
    fprintf(stdout, "init: sequenza completata, esco\n");
    return 0;
}
