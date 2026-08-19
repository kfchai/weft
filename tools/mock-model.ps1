# A deterministic mock model for demos and tests of [W43] infer defs.
# Reads the prompt on stdin, answers with a Weft literal by keyword.
$p = [Console]::In.ReadToEnd()
if ($p -match "sentiment") { "Positive" }
elseif ($p -match "stars") { 'Rating{stars: 4, summary: "solid kettle"}' }
else { '"unknown"' }
