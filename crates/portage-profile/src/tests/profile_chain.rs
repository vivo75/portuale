use super::*;

/// #70 R1 oracle -- the host `cnf/make.globals` fetch-command block
/// (`/usr/share/portage/config/make.globals`, portage 3.0.82.2,
/// lines 63-74 plus line 153) fed through the production
/// [`process_lines`], asserted against `python3 -c "import portage;
/// print(repr(portage.settings[K]))"` on the same host, captured
/// 2026-09-16. Before the fix every value here kept the `\"` escape
/// and had `\${VAR}` wrongly expanded (the #70 bug).
#[test]
fn make_globals_fetch_commands_match_portage_varexpand() {
    let text = r#"FETCHCOMMAND="wget -t 3 -T 60 --passive-ftp -U \"Portage (Gentoo, gentoo.org) distfile-fetch\" -O \"\${DISTDIR}/\${FILE}\" \"\${URI}\""
RESUMECOMMAND="wget -c -t 3 -T 60 --passive-ftp -U \"Portage (Gentoo, gentoo.org) distfile-fetch\" -O \"\${DISTDIR}/\${FILE}\" \"\${URI}\""

FETCHCOMMAND_RSYNC="rsync -LtvP \"\${URI}\" \"\${DISTDIR}/\${FILE}\""
RESUMECOMMAND_RSYNC="rsync -LtvP \"\${URI}\" \"\${DISTDIR}/\${FILE}\""

# NOTE: rsync will evaluate quotes embedded inside PORTAGE_SSH_OPTS
FETCHCOMMAND_SSH="bash -c \"x=\\\${2#ssh://} ; host=\\\${x%%/*} ; port=\\\${host##*:} ; host=\\\${host%:*} ; [[ \\\${host} = \\\${port} ]] && port= ; exec rsync --rsh=\\\"ssh \\\${port:+-p\\\${port}} \\\${3}\\\" -avP \\\"\\\${host}:/\\\${x#*/}\\\" \\\"\\\$1\\\"\" rsync \"\${DISTDIR}/\${FILE}\" \"\${URI}\" \"\${PORTAGE_SSH_OPTS}\""
RESUMECOMMAND_SSH=${FETCHCOMMAND_SSH}

# NOTE: bash eval is used to evaluate quotes embedded inside PORTAGE_SSH_OPTS
FETCHCOMMAND_SFTP="bash -c \"x=\\\${2#sftp://} ; host=\\\${x%%/*} ; port=\\\${host##*:} ; host=\\\${host%:*} ; [[ \\\${host} = \\\${port} ]] && port= ; eval \\\"declare -a ssh_opts=(\\\${3})\\\" ; exec sftp \\\${port:+-P \\\${port}} \\\"\\\${ssh_opts[@]}\\\" \\\"\\\${host}:/\\\${x#*/}\\\" \\\"\\\$1\\\"\" sftp \"\${DISTDIR}/\${FILE}\" \"\${URI}\" \"\${PORTAGE_SSH_OPTS}\""
PORTAGE_GPG_SIGNING_COMMAND="gpg --sign --digest-algo SHA256 --clearsign --yes --default-key \"\${PORTAGE_GPG_KEY}\" --homedir \"\${PORTAGE_GPG_DIR}\" \"\${FILE}\""
"#;
    let mut scalars = HashMap::new();
    let mut config = Config::default();
    process_lines(text, &mut scalars, &mut config, AcceptRawLayer::Globals);
    for (key, want) in [
        (
            "FETCHCOMMAND",
            r#"wget -t 3 -T 60 --passive-ftp -U "Portage (Gentoo, gentoo.org) distfile-fetch" -O "${DISTDIR}/${FILE}" "${URI}""#,
        ),
        (
            "RESUMECOMMAND",
            r#"wget -c -t 3 -T 60 --passive-ftp -U "Portage (Gentoo, gentoo.org) distfile-fetch" -O "${DISTDIR}/${FILE}" "${URI}""#,
        ),
        (
            "FETCHCOMMAND_RSYNC",
            r#"rsync -LtvP "${URI}" "${DISTDIR}/${FILE}""#,
        ),
        (
            "RESUMECOMMAND_RSYNC",
            r#"rsync -LtvP "${URI}" "${DISTDIR}/${FILE}""#,
        ),
        (
            "FETCHCOMMAND_SSH",
            r#"bash -c "x=\${2#ssh://} ; host=\${x%%/*} ; port=\${host##*:} ; host=\${host%:*} ; [[ \${host} = \${port} ]] && port= ; exec rsync --rsh=\"ssh \${port:+-p\${port}} \${3}\" -avP \"\${host}:/\${x#*/}\" \"\$1\"" rsync "${DISTDIR}/${FILE}" "${URI}" "${PORTAGE_SSH_OPTS}""#,
        ),
        (
            "RESUMECOMMAND_SSH",
            r#"bash -c "x=\${2#ssh://} ; host=\${x%%/*} ; port=\${host##*:} ; host=\${host%:*} ; [[ \${host} = \${port} ]] && port= ; exec rsync --rsh=\"ssh \${port:+-p\${port}} \${3}\" -avP \"\${host}:/\${x#*/}\" \"\$1\"" rsync "${DISTDIR}/${FILE}" "${URI}" "${PORTAGE_SSH_OPTS}""#,
        ),
        (
            "FETCHCOMMAND_SFTP",
            r#"bash -c "x=\${2#sftp://} ; host=\${x%%/*} ; port=\${host##*:} ; host=\${host%:*} ; [[ \${host} = \${port} ]] && port= ; eval \"declare -a ssh_opts=(\${3})\" ; exec sftp \${port:+-P \${port}} \"\${ssh_opts[@]}\" \"\${host}:/\${x#*/}\" \"\$1\"" sftp "${DISTDIR}/${FILE}" "${URI}" "${PORTAGE_SSH_OPTS}""#,
        ),
        (
            "PORTAGE_GPG_SIGNING_COMMAND",
            r#"gpg --sign --digest-algo SHA256 --clearsign --yes --default-key "${PORTAGE_GPG_KEY}" --homedir "${PORTAGE_GPG_DIR}" "${FILE}""#,
        ),
    ] {
        assert_eq!(scalars.get(key).map(String::as_str), Some(want), "{key}");
    }
}
