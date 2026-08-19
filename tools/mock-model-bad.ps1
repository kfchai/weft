# A misbehaving mock: replies with an invariant-violating Rating (stars: 9)
# to demonstrate [W42] rejecting model output at the language boundary.
$p = [Console]::In.ReadToEnd()
if ($p -match "sentiment") { "Positive" }
else { 'Rating{stars: 9, summary: "way too enthusiastic"}' }
