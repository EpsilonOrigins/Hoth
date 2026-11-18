{{- define "hoth.name" -}}
{{- printf "%s-hoth" .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- define "hoth.selector" -}}
app.kubernetes.io/name: hoth
app.kubernetes.io/instance: {{ .Release.Name | quote }}
{{- end -}}
{{- define "hoth.pod" -}}
automountServiceAccountToken: false
terminationGracePeriodSeconds: 35
securityContext:
  runAsNonRoot: true
  runAsUser: 10001
  runAsGroup: 10001
  fsGroup: 10001
  seccompProfile:
    type: RuntimeDefault
containers:
  - name: hoth
    image: "{{ .Values.image.repository }}:{{ .Values.image.tag }}"
    imagePullPolicy: {{ .Values.image.pullPolicy }}
    args:
      - run
      - --manifest
      - /artifacts/{{ .Values.storage.manifestPath }}
      - --ready-file
      - /run/hoth/ready
      {{- if .Values.runId }}
      - --run-id
      - $(HOTH_RUN_ID)
      {{- end }}
      {{- if .Values.control.enabled }}
      - --control
      - 0.0.0.0:$(HOTH_CONTROL_PORT)
    ports:
      - name: controller
        protocol: TCP
        containerPort: {{ .Values.control.port }}
      {{- end }}
    envFrom:
      - configMapRef:
          name: {{ include "hoth.name" . }}
    securityContext:
      allowPrivilegeEscalation: false
      readOnlyRootFilesystem: true
      capabilities:
        drop: [ALL]
    resources:
      {{- toYaml .Values.resources | nindent 6 }}
    startupProbe:
      exec:
        command: [/usr/local/bin/hoth, health, --ready-file, /run/hoth/ready]
      periodSeconds: 2
      failureThreshold: 30
    readinessProbe:
      exec:
        command: [/usr/local/bin/hoth, health, --ready-file, /run/hoth/ready]
      periodSeconds: 2
    livenessProbe:
      exec:
        command: [/usr/local/bin/hoth, health, --ready-file, /run/hoth/ready]
      periodSeconds: 3
      failureThreshold: 3
    volumeMounts:
      - {name: artifacts, mountPath: /artifacts, readOnly: true}
      - {name: heartbeat, mountPath: /run/hoth}
volumes:
  - name: artifacts
    persistentVolumeClaim:
      claimName: {{ required "storage.existingClaim is required" .Values.storage.existingClaim | quote }}
      readOnly: true
  - name: heartbeat
    emptyDir: {medium: Memory, sizeLimit: 1Mi}
{{- end -}}
